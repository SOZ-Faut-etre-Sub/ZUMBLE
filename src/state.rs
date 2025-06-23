use crate::channel::Channel;
use crate::client::Client;
use crate::crypt::CryptState;
use crate::error::MumbleError;
use crate::message::ClientMessage;
use crate::proto::mumble::{Authenticate, ChannelRemove, ChannelState, CodecVersion, UserRemove, Version};
use crate::proto::{message_to_bytes, MessageKind};
use crate::sync::RwLock;
use crate::voice::{Serverbound, VoicePacket};
use bytes::BytesMut;
use protobuf::Message;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
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
    pub mute_all: AtomicBool,
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
            mute_all: AtomicBool::new(false),
        }
    }

    pub fn add_client(
        &mut self,
        version: Version,
        authenticate: Authenticate,
        crypt_state: CryptState,
        write: WriteHalf<TlsStream<TcpStream>>,
        publisher: Sender<ClientMessage>,
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

    pub async fn get_client_by_name(&self, name: &str) -> Result<Option<Arc<RwLock<Client>>>, MumbleError> {
        for client in self.clients.values() {
            {
                let client_read = client.read_err().await?;

                if client_read.authenticate.get_username() == name {
                    return Ok(Some(client.clone()));
                }
            }
        }

        Ok(None)
    }

    pub async fn set_client_socket(&mut self, client: Arc<RwLock<Client>>, addr: SocketAddr) -> Result<(), MumbleError> {
        {
            let client_read = client.read_err().await?;

            if let Some(exiting_addr) = client_read.udp_socket_addr {
                self.clients_by_socket.remove(&exiting_addr);
            }
        }

        {
            client.write_err().await?.udp_socket_addr = Some(addr);
        }

        self.clients_by_socket.insert(addr, client);

        Ok(())
    }

    pub async fn broadcast_message<T: Message>(&self, kind: MessageKind, message: &T) -> Result<(), MumbleError> {
        tracing::trace!("broadcast message: {:?}, {:?}", std::any::type_name::<T>(), message);

        let bytes = message_to_bytes(kind, message)?;

        for client in self.clients.values() {
            {
                let client_read = client.read_err().await?;

                match client_read.publisher.try_send(ClientMessage::SendMessage {
                    kind,
                    payload: bytes.clone(),
                }) {
                    Ok(_) => {}
                    Err(err) => {
                        tracing::error!("failed to send message to {}: {}", client_read.authenticate.get_username(), err);
                    }
                }
            }
        }

        Ok(())
    }

    async fn check_leave_channel(&self, leave_channel_id: u32) -> Result<Option<u32>, MumbleError> {
        for client in self.clients.values() {
            {
                let client = client.read_err().await?;

                if client.channel_id.load(Ordering::Relaxed) == leave_channel_id {
                    return Ok(None);
                }
            }
        }

        for channel in self.channels.values() {
            {
                let channel = channel.read_err().await?;

                if channel.parent_id == Some(leave_channel_id) {
                    return Ok(None);
                }
            }
        }

        if let Some(channel) = self.channels.get(&leave_channel_id) {
            {
                let channel = channel.read_err().await?;

                if channel.temporary {
                    // Broadcast channel remove
                    let mut channel_remove = ChannelRemove::new();
                    channel_remove.set_channel_id(leave_channel_id);

                    match self.broadcast_message(MessageKind::ChannelRemove, &channel_remove).await {
                        Ok(_) => (),
                        Err(e) => tracing::error!("failed to send channel remove: {:?}", e),
                    }

                    return Ok(Some(leave_channel_id));
                }
            }

            return Ok(None);
        }

        // Broadcast channel remove
        let mut channel_remove = ChannelRemove::new();
        channel_remove.set_channel_id(leave_channel_id);

        match self.broadcast_message(MessageKind::ChannelRemove, &channel_remove).await {
            Ok(_) => (),
            Err(e) => tracing::error!("failed to send channel remove: {:?}", e),
        }

        Ok(Some(leave_channel_id))
    }

    pub async fn set_client_channel(&self, client: Arc<RwLock<Client>>, channel_id: u32) -> Result<Option<u32>, MumbleError> {
        let leave_channel_id = { client.read_err().await?.join_channel(channel_id) };

        if let Some(leave_channel_id) = leave_channel_id {
            // Broadcast new user state
            let user_state = { client.read_err().await?.get_user_state() };

            match self.broadcast_message(MessageKind::UserState, &user_state).await {
                Ok(_) => (),
                Err(e) => tracing::error!("failed to send user state: {:?}", e),
            }

            return Ok(self.check_leave_channel(leave_channel_id).await?);
        }

        Ok(None)
    }

    pub async fn get_channel_by_name(&self, name: &str) -> Result<Option<Arc<RwLock<Channel>>>, MumbleError> {
        for channel in self.channels.values() {
            {
                let channel_read = channel.read_err().await?;

                if channel_read.name == name {
                    return Ok(Some(channel.clone()));
                }
            }
        }

        Ok(None)
    }

    pub async fn check_codec(&self) -> Result<Option<CodecVersion>, MumbleError> {
        let current_version = { self.codec_state.read_err().await?.get_version() };
        let mut new_version = current_version;
        let mut versions = HashMap::new();

        for client in self.clients.values() {
            {
                let client = client.read_err().await?;

                for version in &client.codecs {
                    *versions.entry(*version).or_insert(0) += 1;
                }
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
            return Ok(Some(self.codec_state.read_err().await?.get_codec_version()));
        }

        let codec_version = {
            let mut codec_state = self.codec_state.write_err().await?;
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
                tracing::error!("failed to broadcast codec version: {:?}", e);
            }
        }

        Ok(None)
    }

    pub fn get_client_by_socket(&self, socket_addr: &SocketAddr) -> Option<Arc<RwLock<Client>>> {
        match self.clients_by_socket.get(socket_addr) {
            Some(client) => Some(client.clone()),
            None => None,
        }
    }

    pub fn remove_client_by_socket(&mut self, socket_addr: &SocketAddr) {
        self.clients_by_socket.remove(socket_addr);
    }

    pub async fn find_client_for_packet(
        &self,
        bytes: &mut BytesMut,
    ) -> Result<(Option<Arc<RwLock<Client>>>, Option<VoicePacket<Serverbound>>, Vec<SocketAddr>), MumbleError> {
        let mut address_to_remove = Vec::new();

        for c in self.clients.values() {
            let crypt_state = { c.read_err().await?.crypt_state.clone() };
            let mut try_buf = bytes.clone();
            let decrypt_result = { crypt_state.write_err().await?.decrypt(&mut try_buf) };

            match decrypt_result {
                Ok(p) => {
                    return Ok((Some(c.clone()), Some(p), address_to_remove));
                }
                Err(err) => {
                    let duration = { Instant::now().duration_since(crypt_state.read_err().await?.last_good).as_millis() };

                    // last good packet was more than 5sec ago, reset
                    if duration > 5000 {
                        let send_crypt_setup = { c.read_err().await?.send_crypt_setup(true).await };

                        if let Err(e) = send_crypt_setup {
                            tracing::error!("failed to send crypt setup: {:?}", e);
                        }

                        let address_option = { c.read_err().await?.udp_socket_addr.clone() };

                        if let Some(address) = address_option {
                            address_to_remove.push(address);

                            {
                                c.write_err().await?.udp_socket_addr = None
                            };
                        }
                    }

                    tracing::debug!("failed to decrypt packet: {:?}, continue to next client", err);
                }
            }
        }

        Ok((None, None, address_to_remove))
    }

    pub async fn disconnect(&mut self, client: Arc<RwLock<Client>>) -> Result<(u32, u32), MumbleError> {
        let client_id = { client.read_err().await?.session_id };

        self.clients.remove(&client_id);

        let socket_addr = { client.read_err().await?.udp_socket_addr.clone() };

        if let Some(socket_addr) = socket_addr {
            self.clients_by_socket.remove(&socket_addr);
        }

        let channel_id = { client.read_err().await?.channel_id.load(Ordering::Relaxed) };

        Ok((client_id, channel_id))
    }

    pub async fn remove_client(&self, client_id: u32, channel_id: u32) -> Result<(), MumbleError> {
        let mut remove = UserRemove::new();
        remove.set_session(client_id);
        remove.set_reason("disconnected".to_string());

        self.broadcast_message(MessageKind::UserRemove, &remove).await?;

        self.check_leave_channel(channel_id).await?;

        Ok(())
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
