use crate::channel::Channel;
use crate::client::Client;
use crate::crypt::CryptState;
use crate::error::MumbleError;
use crate::proto::mumble::{Authenticate, ChannelRemove, ChannelState, CodecVersion, UserRemove, Version};
use crate::proto::{message_to_bytes, MessageKind};
use crate::voice::{Clientbound, Serverbound, VoicePacket};
use bytes::BytesMut;
use futures_util::StreamExt;
use protobuf::Message;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::WriteHalf;
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::mpsc::Sender;
use tokio::sync::RwLock;
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
    pub fn get_version(&self) -> i32 {
        if self.prefer_alpha {
            return self.alpha;
        }

        self.beta
    }

    pub fn get_codec_version(&self) -> CodecVersion {
        let mut codec_version = CodecVersion::default();
        codec_version.set_alpha(self.alpha);
        codec_version.set_beta(self.beta);
        codec_version.set_opus(self.opus);
        codec_version.set_prefer_alpha(self.prefer_alpha);

        codec_version
    }
}

pub struct ServerState {
    pub clients: HashMap<u32, Arc<RwLock<Client>>>,
    pub clients_by_socket: HashMap<SocketAddr, Arc<RwLock<Client>>>,
    pub channels: HashMap<u32, Arc<RwLock<Channel>>>,
    pub codec_state: RwLock<CodecState>,
    pub socket: Arc<UdpSocket>,
}

impl ServerState {
    pub fn new(socket: Arc<UdpSocket>) -> Self {
        let mut channels = HashMap::new();
        channels.insert(
            0,
            Arc::new(RwLock::new(Channel::new(
                0,
                Some(0),
                "Root".to_string(),
                "Root channel".to_string(),
                false,
            ))),
        );

        Self {
            clients: HashMap::new(),
            clients_by_socket: HashMap::new(),
            channels,
            codec_state: RwLock::new(CodecState::default()),
            socket,
        }
    }

    pub fn add_client(
        &mut self,
        version: Version,
        authenticate: Authenticate,
        crypt_state: CryptState,
        write: WriteHalf<TlsStream<TcpStream>>,
        publisher: Sender<VoicePacket<Clientbound>>,
    ) -> Arc<RwLock<Client>> {
        let session_id = self.get_free_session_id();

        let client = Arc::new(RwLock::new(Client::new(
            version,
            authenticate,
            session_id,
            0,
            crypt_state,
            write,
            self.socket.clone(),
            publisher,
        )));

        self.clients.insert(session_id, client.clone());

        client
    }

    pub fn add_channel(&mut self, state: &ChannelState) -> Arc<RwLock<Channel>> {
        let channel_id = self.get_free_channel_id();
        let channel = Arc::new(RwLock::new(Channel::new(
            channel_id,
            Some(state.get_parent()),
            state.get_name().to_string(),
            state.get_description().to_string(),
            state.get_temporary(),
        )));

        self.channels.insert(channel_id, channel.clone());

        channel
    }

    pub async fn get_client_by_name(&self, name: &str) -> Option<Arc<RwLock<Client>>> {
        for client in self.clients.values() {
            if client.read().await.authenticate.get_username() == name {
                return Some(client.clone());
            }
        }

        None
    }

    pub async fn set_client_socket(&mut self, client: Arc<RwLock<Client>>, addr: SocketAddr) {
        if let Some(exiting_addr) = client.read().await.udp_socket_addr {
            self.clients_by_socket.remove(&exiting_addr);
        }

        {
            client.write().await.udp_socket_addr = Some(addr);
        }

        self.clients_by_socket.insert(addr, client);
    }

    pub async fn broadcast_message<T: Message>(&self, kind: MessageKind, message: &T) -> Result<(), MumbleError> {
        log::trace!("broadcast message: {:?}, {:?}", std::any::type_name::<T>(), message);

        let bytes = message_to_bytes(kind, message)?;
        let cursor = bytes.as_ref();

        futures_util::stream::iter(self.clients.values())
            .for_each_concurrent(None, |client| async move {
                {
                    match client.clone().write().await.send(cursor).await {
                        Ok(_) => (),
                        Err(e) => log::error!("failed to send message: {:?}", e),
                    }
                }
            })
            .await;

        Ok(())
    }

    async fn check_leave_channel(&self, leave_channel_id: u32) -> Option<u32> {
        for client in self.clients.values() {
            if client.read().await.channel_id == leave_channel_id {
                return None;
            }
        }

        for channel in self.channels.values() {
            if let Some(parent_id) = channel.read().await.parent_id {
                if parent_id == leave_channel_id {
                    return None;
                }
            }
        }

        if let Some(channel) = self.channels.get(&leave_channel_id) {
            if channel.read().await.temporary {
                // Broadcast channel remove
                let mut channel_remove = ChannelRemove::new();
                channel_remove.set_channel_id(leave_channel_id);

                match self.broadcast_message(MessageKind::ChannelRemove, &channel_remove).await {
                    Ok(_) => (),
                    Err(e) => log::error!("failed to send channel remove: {:?}", e),
                }

                return Some(leave_channel_id);
            }

            return None;
        }

        // Broadcast channel remove
        let mut channel_remove = ChannelRemove::new();
        channel_remove.set_channel_id(leave_channel_id);

        match self.broadcast_message(MessageKind::ChannelRemove, &channel_remove).await {
            Ok(_) => (),
            Err(e) => log::error!("failed to send channel remove: {:?}", e),
        }

        return Some(leave_channel_id);
    }

    pub async fn set_client_channel(&self, client: Arc<RwLock<Client>>, channel_id: u32) -> Result<Option<u32>, MumbleError> {
        let leave_channel_id = { client.write().await.join_channel(channel_id) };

        if let Some(leave_channel_id) = leave_channel_id {
            // Broadcast new user state
            let user_state = { client.read().await.get_user_state() };

            match self.broadcast_message(MessageKind::UserState, &user_state).await {
                Ok(_) => (),
                Err(e) => log::error!("failed to send user state: {:?}", e),
            }

            return Ok(self.check_leave_channel(leave_channel_id).await);
        }

        Ok(None)
    }

    pub async fn get_channel_by_name(&self, name: &str) -> Option<Arc<RwLock<Channel>>> {
        for channel in self.channels.values() {
            if channel.read().await.name.as_str() == name {
                return Some(channel.clone());
            }
        }

        None
    }

    pub async fn check_codec(&self) -> Option<CodecVersion> {
        let current_version = { self.codec_state.read().await.get_version() };
        let mut new_version = current_version;
        let mut versions = HashMap::new();

        for client in self.clients.values() {
            for version in &client.read().await.codecs {
                *versions.entry(*version).or_insert(0) += 1;
            }
        }

        let mut max = 0;

        for (version, count) in versions {
            if count > max {
                new_version = version;
                max = count;
            }
        }

        if new_version == current_version {
            return Some(self.codec_state.read().await.get_codec_version());
        }

        let codec_version = {
            let mut codec_state = self.codec_state.write().await;
            codec_state.prefer_alpha = !codec_state.prefer_alpha;

            if codec_state.prefer_alpha {
                codec_state.alpha = new_version;
            } else {
                codec_state.beta = new_version;
            }

            codec_state.get_codec_version()
        };

        match self.broadcast_message(MessageKind::CodecVersion, &codec_version).await {
            Ok(_) => (),
            Err(e) => {
                log::error!("failed to broadcast codec version: {:?}", e);
            }
        }

        None
    }

    pub fn get_client_by_socket(&self, socket_addr: &SocketAddr) -> Option<Arc<RwLock<Client>>> {
        self.clients_by_socket.get(socket_addr).cloned()
    }

    pub async fn find_client_for_packet(&self, bytes: &mut BytesMut) -> (Option<Arc<RwLock<Client>>>, Option<VoicePacket<Serverbound>>) {
        let mut client = None;
        let mut packet = None;

        for c in self.clients.values() {
            let client_read = c.read().await;

            let mut crypt_state = client_read.crypt_state.write().await;
            let mut try_buf = bytes.clone();

            match crypt_state.decrypt(&mut try_buf) {
                Ok(p) => {
                    packet = Some(p);
                    client = Some(c.clone());
                }
                Err(err) => {
                    log::debug!("failed to decrypt packet: {:?}, continue to next client", err);

                    continue;
                }
            }
        }

        (client, packet)
    }

    pub async fn disconnect(&mut self, client: Arc<RwLock<Client>>) {
        self.clients.remove(&client.read().await.session_id);

        if let Some(socket_addr) = client.read().await.udp_socket_addr {
            self.clients_by_socket.remove(&socket_addr);
        }

        let client_id = { client.read().await.session_id };

        for channel in self.channels.values() {
            channel.write().await.listeners.remove(&client_id);
        }

        for client in self.clients.values() {
            for target in &client.read().await.targets {
                target.write().await.sessions.remove(&client_id);
            }
        }

        let mut remove = UserRemove::new();
        remove.set_session(client_id);
        remove.set_reason("disconnected".to_string());

        self.broadcast_message(MessageKind::UserRemove, &remove).await.unwrap();

        self.check_leave_channel(client.read().await.channel_id).await;
    }

    fn get_free_session_id(&self) -> u32 {
        let mut session_id = 1;

        loop {
            if self.clients.contains_key(&session_id) {
                session_id += 1;
            } else {
                break;
            }
        }

        session_id
    }

    fn get_free_channel_id(&self) -> u32 {
        let mut channel_id = 1;

        loop {
            if self.channels.contains_key(&channel_id) {
                channel_id += 1;
            } else {
                break;
            }
        }

        channel_id
    }
}
