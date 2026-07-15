use crate::channel::{Channel, ChannelRef, WeakChannelRef};
use crate::client::{Client, ClientArc, WeakClient};
use crate::crypt::CryptState;
use crate::error::{DisconnectReason, MumbleError};
use crate::message::ClientMessage;
use crate::metrics::DISCONNECT;
use crate::proto::mumble::{Authenticate, ChannelRemove, ChannelState, CodecVersion, UserRemove, Version};
use crate::proto::{MessageKind, message_to_bytes};
use crate::server::constants::{ConcurrentHashMap, MAX_CLIENTS};
use crate::voice::{ServerBound, VoicePacket};
use bytes::BytesMut;
use protobuf::Message;
// use scc::HashCache;
use scc::ebr::Guard;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use tokio::io::WriteHalf;
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::mpsc::Sender;
use tokio_rustls::server::TlsStream;

pub struct CodecState {
    pub opus: bool,
    pub alpha: i32,
    pub beta: i32,
    pub prefer_alpha: bool,
}

impl Default for CodecState {
    fn default() -> Self {
        Self {
            opus: true,
            alpha: 0,
            beta: 0,
            prefer_alpha: false,
        }
    }
}

impl CodecState {
    // pub fn get_version(&self) -> i32 {
    //     if self.prefer_alpha {
    //         return self.alpha;
    //     }

    //     self.beta
    // }

    pub fn get_codec_version(&self) -> CodecVersion {
        let mut codec_version = CodecVersion::default();
        codec_version.set_alpha(self.alpha);
        codec_version.set_beta(self.beta);
        codec_version.set_opus(self.opus);
        codec_version.set_prefer_alpha(self.prefer_alpha);

        codec_version
    }
}

pub type ServerStateRef = Arc<ServerState>;

pub struct ServerState {
    pub remove_positional_data: bool,
    pub clients: ConcurrentHashMap<u32, ClientArc>,
    pub clients_without_udp: ConcurrentHashMap<u32, WeakClient>,
    pub clients_by_socket: ConcurrentHashMap<SocketAddr, WeakClient>,
    // pub clients_by_peer: ConcurrentHashMap<IpAddr, AtomicU32>,
    pub channels: ConcurrentHashMap<u32, ChannelRef>,
    pub disconnect_queue: ConcurrentHashMap<u32, DisconnectReason>,
    pub codec_state: Arc<CodecState>,
    pub restrict_to_version: Arc<Option<String>>,
    // used only for logging
    pub debug_message_id: AtomicU64,
    // pub logs: HashCache<SocketAddr, ()>,
    session_count: AtomicU32,
    channel_count: AtomicU32,
    pub active_clients: AtomicU32,
}

impl ServerState {
    pub fn new(remove_positional_data: bool, restrict_to_version: Option<String>) -> Self {
        let channels = ConcurrentHashMap::new();
        let _ = channels.insert(0, Channel::new(0, Some(0), "Root".to_string(), "Root channel".to_string(), false));

        Self {
            remove_positional_data,
            // we preallocate the maximum amount of clients to prevent the possibility of resizes
            // later, which will prevent double-sends in certain situations
            clients: ConcurrentHashMap::with_capacity(MAX_CLIENTS),
            restrict_to_version: Arc::new(restrict_to_version.map(|v| v.to_lowercase())),
            // logs: HashCache::with_capacity(500, 1000),
            clients_without_udp: ConcurrentHashMap::with_capacity(MAX_CLIENTS),
            clients_by_socket: ConcurrentHashMap::with_capacity(MAX_CLIENTS),
            disconnect_queue: ConcurrentHashMap::with_capacity(MAX_CLIENTS),
            // clients_by_peer: ConcurrentHashMap::with_capacity(MAX_CLIENTS),
            channels,
            codec_state: Arc::new(CodecState::default()),
            debug_message_id: AtomicU64::new(0),
            session_count: AtomicU32::new(1),
            channel_count: AtomicU32::new(1),
            active_clients: AtomicU32::new(0),
        }
    }

    pub async fn add_client(
        &self,
        version: Version,
        authenticate: Authenticate,
        crypt_state: CryptState,
        write: WriteHalf<TlsStream<TcpStream>>,
        publisher: Sender<ClientMessage>,
        _peer_ip: IpAddr,
    ) -> ClientArc {
        let session_id = self.get_free_session_id();

        let client = Client::new(version, authenticate, session_id, 0, crypt_state, write, publisher);

        crate::metrics::CLIENTS_TOTAL.inc();
        self.active_clients.fetch_add(1, Ordering::Relaxed);
        let _ = self.clients.insert_async(session_id, Arc::clone(&client)).await;
        // if let Some(ref_count) = self.clients_by_peer.get(&peer_ip) {
        //     ref_count.fetch_add(1, Ordering::SeqCst);
        // } else {
        //     self.clients_by_peer.upsert_async(peer_ip, AtomicU32::new(1)).await;
        // }

        let _ = self.clients_without_udp.insert_async(session_id, Arc::downgrade(&client)).await;

        client
    }

    pub async fn add_channel(&self, state: &ChannelState) -> ChannelRef {
        let channel_id = self.get_free_channel_id();
        let channel = Channel::new(
            channel_id,
            Some(state.get_parent()),
            state.get_name().to_string(),
            state.get_description().to_string(),
            state.get_temporary(),
        );

        tracing::debug!("Created channel {} with name {}", channel_id, state.get_name().to_string());

        // this should already be checked prior to us creating the channel
        let _ = self.channels.insert_async(channel_id, Arc::clone(&channel)).await;

        channel
    }

    pub async fn get_client_by_name(&self, name: &str) -> Option<ClientArc> {
        let client = self
            .clients
            .any_entry_async(|_k, client| client.authenticate.get_username() == name)
            .await;

        if let Some(cl) = client {
            return Some(Arc::clone(cl.get()));
        }

        None
    }

    pub async fn set_client_socket(&self, client: &ClientArc, udp_socket: Arc<UdpSocket>, addr: SocketAddr) {
        client.outbound_udp_socket.store(Some(udp_socket));
        let socket_lock = client.udp_socket_addr.swap(Some(Arc::new(addr)));
        if let Some(exiting_addr) = socket_lock {
            self.clients_by_socket.remove_async(exiting_addr.as_ref()).await;
        }

        let _ = self.clients_by_socket.insert_async(addr, Arc::downgrade(client)).await;
    }

    pub fn add_client_to_disconnect_queue(&self, session_id: u32, disconnect_reason: DisconnectReason) {
        // if we fail to add the session to the queue we don't care.
        let _ = self.disconnect_queue.insert(session_id, disconnect_reason);
    }

    pub fn broadcast_message<T: Message>(&self, kind: MessageKind, message: &T) -> Result<(), MumbleError> {
        let message_id = self.debug_message_id.fetch_add(1, Ordering::Relaxed);
        tracing::trace!(
            "[message_id: {message_id}] broadcast message: {:?}, {:?}",
            std::any::type_name::<T>(),
            message
        );

        let bytes = message_to_bytes(kind, message)?;

        let guard = Guard::new();

        for (_k, client) in self.clients.iter(&guard) {
            let _ = client
                .publisher
                .try_send(ClientMessage::SendMessage {
                    kind,
                    payload: bytes.clone(),
                })
                .map_err(|_e| {
                    self.add_client_to_disconnect_queue(client.session_id, DisconnectReason::ClientMSPCFull);
                });
        }

        Ok(())
    }

    async fn handle_client_left_channel(&self, client_session: u32, leave_channel_id: u32) -> Option<u32> {
        {
            let channel = self.channels.get_async(&leave_channel_id).await;
            if let Some(chan) = channel {
                let c = chan.get();

                // remove the client from the channel
                c.clients.remove_async(&client_session).await;

                // if the channel isn't temporary then we want to keep it
                if !c.temporary || !c.get_clients().is_empty() {
                    return None;
                };
            }
        }

        tracing::info!("Deleting channel {leave_channel_id} because session: {client_session} was the only client in it.");

        // Broadcast channel remove
        let mut channel_remove = ChannelRemove::new();
        channel_remove.set_channel_id(leave_channel_id);

        self.channels.remove_async(&leave_channel_id).await;

        match self.broadcast_message(MessageKind::ChannelRemove, &channel_remove) {
            Ok(_) => (),
            Err(e) => tracing::error!("failed to send channel remove: {:?}", e),
        }

        Some(leave_channel_id)
    }

    pub async fn set_client_channel(&self, client: &ClientArc, channel_id: u32) -> Result<(), MumbleError> {
        let leave_channel_id = client.join_channel(channel_id);

        tracing::info!(
            "Client: {} joined channel {} and left channel {:?}",
            client.session_id,
            channel_id,
            leave_channel_id
        );

        {
            {
                let mut was_set = false;

                let mut iter = self.channels.first_entry_async().await;
                while let Some(channel) = iter {
                    let c = channel.get();

                    if c.id == channel_id {
                        // remove the client from the channel
                        let _ = c.clients.insert_async(client.session_id, Arc::clone(client)).await;
                        was_set = true;

                        break;
                    }

                    iter = channel.next_async().await;
                }

                if !was_set {
                    return Err(MumbleError::ChannelDoesntExist);
                }
            }
        }

        // Broadcast new user state
        let user_state = client.get_user_state();
        match self.broadcast_message(MessageKind::UserState, &user_state) {
            Ok(_) => (),
            Err(e) => tracing::error!("failed to send user state: {:?}", e),
        }

        if let Some(leave_channel_id) = leave_channel_id {
            // if the channel we're joining is the same channel we dont want to do leave logic
            if leave_channel_id == channel_id {
                return Ok(());
            };
            self.handle_client_left_channel(client.session_id, leave_channel_id).await;
        }

        Ok(())
    }

    pub async fn get_channel_by_channel_id(&self, channel_id: u32) -> Option<WeakChannelRef> {
        self.channels
            .any_entry_async(|_k, channel| channel.id == channel_id)
            .await
            .map_or(None, |channel| Some(Arc::downgrade(channel.get())))
    }

    pub async fn get_channel_by_name(&self, name: &str) -> Option<WeakChannelRef> {
        self.channels
            .any_entry_async(|_k, channel| channel.name == name)
            .await
            .map_or(None, |channel| Some(Arc::downgrade(channel.get())))
    }

    pub async fn get_client_by_session_id(&self, session_id: u32) -> Option<WeakClient> {
        self.clients
            .get_async(&session_id)
            .await
            .map_or(None, |client| Some(Arc::downgrade(client.get())))
    }

    pub async fn get_client_by_socket(&self, socket_addr: &SocketAddr) -> Option<ClientArc> {
        self.clients_by_socket
            .get_async(socket_addr)
            .await
            .and_then(|client| client.get().upgrade())
    }

    pub async fn remove_client_by_socket(&self, socket_addr: &SocketAddr) -> bool {
        self.clients_by_socket.remove_async(socket_addr).await
    }

    pub async fn find_client_with_decrypt(
        &self,
        socket: &Arc<UdpSocket>,
        bytes: &mut BytesMut,
        addr: SocketAddr,
    ) -> Result<Option<(ClientArc, VoicePacket<ServerBound>)>, MumbleError> {
        let mut client_and_packet = None;

        let mut iter = self.clients_without_udp.first_entry_async().await;

        while let Some(client) = iter {
            let c = client.get();
            if let Some(c) = c.upgrade() {
                let mut try_buf = bytes.clone();
                let decrypt_result = {
                    let mut crypt_state = c.crypt_state.lock();
                    crypt_state.decrypt(&mut try_buf)
                };

                match decrypt_result {
                    Ok(p) => {
                        self.set_client_socket(&c, socket.clone(), addr).await;
                        client_and_packet = Some((c, p));
                        break;
                    }
                    Err(err) => {
                        tracing::debug!("failed to decrypt packet: {:?}, continue to next client", err);
                    }
                }
            }

            iter = client.next_async().await;
        }

        if let Some((client, _)) = &client_and_packet {
            self.clients_without_udp.remove_async(&client.session_id).await;
        }

        Ok(client_and_packet)
    }

    /// NOTE: This shouldn't be called in an iterator for `client_by_socket` or else it will cause
    /// a deadlock
    ///
    /// Resets the clients crypt state and removes their udp socket so we no longer take invalid
    /// data from the UDP stream
    pub async fn reset_client_crypt(&self, client: &ClientArc) -> Result<(), MumbleError> {
        let _ = self
            .clients_without_udp
            .insert_async(client.session_id, Arc::downgrade(client))
            .await;

        // swap out the clients socket with none so we don't try to reuse the old socket
        let address_option = client.remove_udp_socket();

        if let Some(address) = address_option {
            // remove the socket
            self.remove_client_by_socket(&address).await;
        }

        client.send_crypt_setup(true).await
    }

    pub async fn disconnect(&self, client_session: u32, disconnect_reason: DisconnectReason) {
        let mut channel_id = None;

        // Grab the client before trying to call any of the disconnect code, and make sure
        // that the call to `self.client` returns `true` (the client still exists)
        // before we call any of the `active_clients` code to prevent underflowing the u32
        //
        // This fixes [GH-12](https://github.com/AvarianKnight/rust-mumble/issues/12)
        //
        // Which causes the `active_clients` to get double decremented and overflow,
        // if we manage to hit the disconnect perfectly so that two threads race
        // the deletion, which can happen with auto-cleanup, since this will call
        // `cancel_token`, which also causes the main client loop to call `disconnect`
        let client = self.get_client_by_session_id(client_session).await.and_then(|c| c.upgrade());

        if self.clients.remove_async(&client_session).await
            && let Some(client) = client
        {
            channel_id = Some(client.channel_id.load(Ordering::Relaxed));
            self.clients_without_udp.remove_async(&client_session).await;

            crate::metrics::CLIENTS_TOTAL.dec();
            self.active_clients.fetch_sub(1, Ordering::Relaxed);
            DISCONNECT.with_label_values(&[&disconnect_reason.to_string()]).inc();
            tracing::info!("Removing client {} with reason {:?}", client, disconnect_reason);

            // tell the client loop to shut down their UDP/TCP threads, this will drop the
            // reader part of the TCP stream
            client.cancel_token.cancel();

            // Shut down our writer whenever we get disconnected, allowing for the TCP stream
            // to shut down
            //
            // This is required due to the fact that `HashIndex` doesn't guarantee a stable
            // garbage collection, so we can have a client exist for a long time afterwards
            // which will cause their socket to not close until we eventually hit GC, which would
            // increase memory usage, and also cause us to hit our socket limit.
            let client_shutdown = Arc::clone(&client);
            tokio::task::spawn(async move {
                let mut client_writer = client_shutdown.write.lock().await;

                // take the writer so we can drop it
                client_writer.take();
            });

            let socket = client.udp_socket_addr.swap(None);

            if let Some(socket_addr) = socket {
                self.remove_client_by_socket(&socket_addr).await;
            }

            if let Some(channel_id) = channel_id {
                self.broadcast_client_delete(client_session, channel_id).await;
            }
        }

        if let Some(channel_id) = channel_id {
            let channel = self.get_channel_by_channel_id(channel_id).await;
            if let Some(c) = channel
                && let Some(channel) = c.upgrade()
            {
                channel.clients.retain_async(|session_id, _| *session_id != client_session).await;
            }
        }
    }

    async fn broadcast_client_delete(&self, client_id: u32, channel_id: u32) {
        let mut remove = UserRemove::new();
        remove.set_session(client_id);
        remove.set_reason("Disconnected".to_string());

        let _ = self.broadcast_message(MessageKind::UserRemove, &remove);

        self.handle_client_left_channel(client_id, channel_id).await;
    }

    /// Gets a free session id for a joining client to use
    ///
    /// This can loop whenenver (in the unlikely case) the server session ids have overflowed
    fn get_free_session_id(&self) -> u32 {
        let mut session_id = self.session_count.fetch_add(1, Ordering::SeqCst);

        while self.clients.contains(&session_id) {
            session_id = self.session_count.fetch_add(1, Ordering::SeqCst);
        }

        session_id
    }

    /// Gets a free channel id for a channel to use
    ///
    /// This can loop whenever (in the unlikely case) the server session ids have overflowed
    fn get_free_channel_id(&self) -> u32 {
        let mut channel_id = self.channel_count.fetch_add(1, Ordering::SeqCst);

        while self.channels.contains(&channel_id) {
            channel_id = self.channel_count.fetch_add(1, Ordering::SeqCst);
        }

        channel_id
    }
}
