use crate::channel::{Channel, ChannelRef, WeakChannelRef};
use crate::client::{Client, ClientArc, WeakClient};
use crate::crypt::CryptState;
use crate::error::{DisconnectReason, MumbleError};
use crate::message::ClientMessage;
use crate::proto::mumble::{Authenticate, ChannelRemove, ChannelState, CodecVersion, UserRemove, Version};
use crate::proto::{message_to_bytes, MessageKind};
use crate::server::constants::{ConcurrentHashMap, MAX_CLIENTS};
use crate::voice::{ServerBound, VoicePacket};
use bytes::BytesMut;
use protobuf::Message;
use scc::ebr::Guard;
use scc::HashCache;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Weak};
use tokio::io::{AsyncWriteExt, WriteHalf};
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
    pub clients: ConcurrentHashMap<u32, ClientArc>,
    pub clients_without_udp: ConcurrentHashMap<u32, ClientArc>,
    pub clients_by_socket: ConcurrentHashMap<SocketAddr, ClientArc>,
    // pub clients_by_peer: ConcurrentHashMap<IpAddr, AtomicU32>,
    pub channels: ConcurrentHashMap<u32, ChannelRef>,
    pub codec_state: Arc<CodecState>,
    pub socket: Arc<UdpSocket>,
    pub logs: HashCache<SocketAddr, ()>,
    session_count: AtomicU32,
    channel_count: AtomicU32,
}

impl ServerState {
    pub fn new(socket: Arc<UdpSocket>) -> Self {
        let channels = ConcurrentHashMap::new();
        channels.insert(0, Channel::new(0, Some(0), "Root".to_string(), "Root channel".to_string(), false));

        Self {
            // we preallocate the maximum amount of clients to prevent the possibility of resizes
            // later, which will prevent double-sends in certain situations
            clients: ConcurrentHashMap::with_capacity(MAX_CLIENTS),
            logs: HashCache::with_capacity(500, 1000),
            clients_without_udp: ConcurrentHashMap::with_capacity(MAX_CLIENTS),
            clients_by_socket: ConcurrentHashMap::with_capacity(MAX_CLIENTS),
            // clients_by_peer: ConcurrentHashMap::with_capacity(MAX_CLIENTS),
            channels,
            codec_state: Arc::new(CodecState::default()),
            socket,
            session_count: AtomicU32::new(1),
            channel_count: AtomicU32::new(1),
        }
    }

    pub fn add_client(
        &self,
        version: Version,
        authenticate: Authenticate,
        crypt_state: CryptState,
        write: WriteHalf<TlsStream<TcpStream>>,
        publisher: Sender<ClientMessage>,
        _peer_ip: IpAddr,
    ) -> ClientArc {
        let session_id = self.get_free_session_id();

        let client = Client::new(
            version,
            authenticate,
            session_id,
            0,
            crypt_state,
            write,
            Arc::clone(&self.socket),
            publisher,
        );

        crate::metrics::CLIENTS_TOTAL.inc();
        let _ = self.clients.insert(session_id, Arc::clone(&client));
        // if let Some(ref_count) = self.clients_by_peer.get(&peer_ip) {
        //     ref_count.fetch_add(1, Ordering::SeqCst);
        // } else {
        //     self.clients_by_peer.upsert_async(peer_ip, AtomicU32::new(1)).await;
        // }

        let _ = self.clients_without_udp.insert(session_id, Arc::clone(&client));

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

        self.channels.insert(channel_id, Arc::clone(&channel));

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

    pub async fn set_client_socket(&self, client: &ClientArc, addr: SocketAddr) {
        let socket_lock = client.udp_socket_addr.swap(Some(Arc::new(addr)));
        if let Some(exiting_addr) = socket_lock {
            self.clients_by_socket.remove_async(exiting_addr.as_ref()).await;
        }

        self.clients_by_socket.insert_async(addr, Arc::clone(client)).await;
    }

    pub fn broadcast_message<T: Message>(&self, kind: MessageKind, message: &T) -> Result<(), MumbleError> {
        tracing::trace!("broadcast message: {:?}, {:?}", std::any::type_name::<T>(), message);

        let bytes = message_to_bytes(kind, message)?;

        let bytes = Arc::new(bytes);

        let guard = Guard::new();

        for (_k, client) in self.clients.iter(&guard) {
            match client.publisher.try_send(ClientMessage::SendMessage {
                kind,
                payload: Arc::clone(&bytes),
            }) {
                Ok(_) => {}
                Err(err) => {
                    tracing::error!("failed to send message to {}: {}", client, err);
                }
            };
        }

        Ok(())
    }

    fn handle_client_left_channel(&self, client_session: u32, leave_channel_id: u32) -> Option<u32> {
        {
            let guard = Guard::new();
            if let Some(channel) = self.channels.peek(&leave_channel_id, &guard) {
                // remove the client from the channel
                channel.clients.remove(&client_session);

                channel.parent_id?;

                // if the channel isn't temporary then we want to keep it
                if !channel.temporary || !channel.get_clients().is_empty() {
                    return None;
                };
            }
        }

        // Broadcast channel remove
        let mut channel_remove = ChannelRemove::new();
        channel_remove.set_channel_id(leave_channel_id);

        self.channels.remove(&leave_channel_id);

        match self.broadcast_message(MessageKind::ChannelRemove, &channel_remove) {
            Ok(_) => (),
            Err(e) => tracing::error!("failed to send channel remove: {:?}", e),
        }

        Some(leave_channel_id)
    }

    pub async fn set_client_channel(&self, client: &ClientArc, channel: u32) -> Result<(), MumbleError> {
        let leave_channel_id = client.join_channel(channel);

        tracing::info!(
            "Client: {} joined channel {} and left channel {:?}",
            client.session_id,
            channel,
            leave_channel_id
        );

        {
            let guard = Guard::new();
            if let Some(channel) = self.channels.peek(&channel, &guard) {
                let _ = channel.clients.insert(client.session_id, Arc::clone(client));
            } else {
                return Err(MumbleError::ChannelDoesntExist);
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
            if leave_channel_id == channel {
                return Ok(());
            };
            self.handle_client_left_channel(client.session_id, leave_channel_id);
        }

        Ok(())
    }

    pub async fn get_channel_by_name(&self, name: &str) -> Option<WeakChannelRef> {
        let client = self.channels.any_entry_async(|_k, channel| channel.name == name).await;

        if let Some(cl) = client {
            return Some(Arc::downgrade(cl.get()));
        }

        None
    }

    pub async fn get_client_by_socket(&self, socket_addr: &SocketAddr) -> Option<ClientArc> {
        self.clients_by_socket
            .get_async(socket_addr)
            .await
            .map(|client| Arc::clone(client.get()))
    }

    pub fn remove_client_by_socket(&self, socket_addr: &SocketAddr) -> bool {
        self.clients_by_socket.remove(socket_addr)
    }

    pub async fn find_client_with_decrypt(
        &self,
        bytes: &mut BytesMut,
        addr: SocketAddr,
    ) -> Result<Option<(ClientArc, VoicePacket<ServerBound>)>, MumbleError> {
        let mut client_and_packet = None;

        let mut iter = self.clients_without_udp.first_entry_async().await;

        while let Some(client) = iter {
            let c = client.get();
            let mut try_buf = bytes.clone();
            let decrypt_result = {
                let mut crypt_state = c.crypt_state.lock().await;
                crypt_state.decrypt(&mut try_buf)
            };

            match decrypt_result {
                Ok(p) => {
                    self.set_client_socket(c, addr).await;
                    client_and_packet = Some((Arc::clone(c), p));
                    break;
                }
                Err(err) => {
                    tracing::debug!("failed to decrypt packet: {:?}, continue to next client", err);
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
        let _ = self.clients_without_udp.insert(client.session_id, Arc::clone(client));

        // swap out the clients socket with none so we don't try to reuse the old socket
        let address_option = client.remove_udp_socket();

        if let Some(address) = address_option {
            // remove the socket
            self.remove_client_by_socket(&address);
        }

        client.send_crypt_setup(true).await
    }

    pub async fn disconnect(&self, client_session: u32, disconnect_reason: DisconnectReason) {
        // if the client was listening to any channels we want to remove them
        {
            let guard = Guard::new();
            for (_, channel) in self.channels.iter(&guard) {
                channel.listeners.retain(|session_id, _| *session_id != client_session);
            }
        }

        let mut channel = None;

        {
            let guard = Guard::new();
            let client = self.clients.peek(&client_session, &guard);

            if let Some(client) = client {
                crate::metrics::CLIENTS_TOTAL.dec();
                tracing::info!("Removing client {} with reason {:?}", client, disconnect_reason);

                // tell the client loop to shut down their UDP/TCP threads
                client.cancel_token.cancel();

                let socket = client.udp_socket_addr.swap(None);
                // let mut should_remove = false;

                if let Some(socket_addr) = socket {
                    self.remove_client_by_socket(&socket_addr);
                    // if let Some(ref_count) = self.clients_by_peer.get(&socket_addr.ip()) {
                    //     let count = ref_count.fetch_sub(1, Ordering::SeqCst);
                    //     // if our last count was 0 that means our new count will be 0, we should remove them from the map
                    //     should_remove = count == 1;
                    // }
                    //
                    // if should_remove {
                    //     self.clients_by_peer.remove(&socket_addr.ip());
                    // }
                }

                channel = Some(client.channel_id.load(Ordering::Relaxed));
            }
        }

        // TODO: Figure out if this is needed whenever we are already deleting the client
        if let Some(channel_id) = channel {
            self.broadcast_client_delete(client_session, channel_id).await;
        }

        self.clients.remove(&client_session);
        self.clients_without_udp.remove(&client_session);
    }

    async fn broadcast_client_delete(&self, client_id: u32, channel_id: u32) {
        let mut remove = UserRemove::new();
        remove.set_session(client_id);
        remove.set_reason("disconnected".to_string());

        let _ = self.broadcast_message(MessageKind::UserRemove, &remove);

        self.handle_client_left_channel(client_id, channel_id);
    }

    // TODO: this can still wrap and overwrite existing sessions, though its very unlikely
    fn get_free_session_id(&self) -> u32 {
        self.session_count.fetch_add(1, Ordering::SeqCst)
    }

    fn get_free_channel_id(&self) -> u32 {
        self.channel_count.fetch_add(1, Ordering::SeqCst)
    }
}
