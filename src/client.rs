use crate::crypt::CryptState;
use crate::error::MumbleError;
use crate::proto::mumble::{Authenticate, ServerConfig, ServerSync, UserState, Version};
use crate::proto::{expected_message, message_to_bytes, send_message, MessageKind};
use crate::sync::RwLock;
use crate::target::VoiceTarget;
use crate::voice::{encode_voice_packet, Clientbound, VoicePacket};
use crate::ServerState;
use bytes::{BufMut, BytesMut};
use protobuf::Message;
use std::net::SocketAddr;
use std::ops::DerefMut;
use std::sync::Arc;
use std::time::Instant;
use tokio::io::{AsyncWriteExt, WriteHalf};
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::mpsc::Sender;
use tokio_rustls::server::TlsStream;

pub struct Client {
    pub version: Version,
    pub authenticate: Authenticate,
    pub session_id: u32,
    pub channel_id: u32,
    pub mute: bool,
    pub deaf: bool,
    pub write: RwLock<WriteHalf<TlsStream<TcpStream>>>,
    pub tokens: Vec<String>,
    pub crypt_state: Arc<RwLock<CryptState>>,
    pub udp_socket_addr: Option<SocketAddr>,
    pub use_opus: bool,
    pub codecs: Vec<i32>,
    pub udp_socket: Arc<UdpSocket>,
    pub publisher: Sender<VoicePacket<Clientbound>>,
    pub publisher_disconnect: Sender<bool>,
    pub targets: Vec<Arc<RwLock<VoiceTarget>>>,
    pub last_ping: RwLock<Instant>,
}

impl Client {
    pub async fn init(
        stream: &mut TlsStream<TcpStream>,
        server_version: Version,
    ) -> Result<(Version, Authenticate, CryptState), MumbleError> {
        let version: Version = expected_message(MessageKind::Version, stream, 0).await?;

        // Send version
        send_message(MessageKind::Version, &server_version, stream).await?;

        // Get authenticate
        let authenticate: Authenticate = expected_message(MessageKind::Authenticate, stream, 0).await?;

        let crypt = CryptState::default();
        let crypt_setup = crypt.get_crypt_setup();

        // Send crypt setup
        send_message(MessageKind::CryptSetup, &crypt_setup, stream).await?;

        Ok((version, authenticate, crypt))
    }

    pub fn new(
        version: Version,
        authenticate: Authenticate,
        session_id: u32,
        channel_id: u32,
        crypt_state: CryptState,
        write: WriteHalf<TlsStream<TcpStream>>,
        udp_socket: Arc<UdpSocket>,
        publisher: Sender<VoicePacket<Clientbound>>,
        publisher_disconnect: Sender<bool>,
    ) -> Self {
        let tokens = authenticate.get_tokens().iter().map(|token| token.to_string()).collect();
        let mut targets = Vec::with_capacity(30);
        targets.resize_with(30, Default::default);

        Self {
            version,
            session_id,
            channel_id,
            crypt_state: Arc::new(RwLock::new(crypt_state)),
            write: RwLock::new(write),
            tokens,
            deaf: false,
            mute: false,
            udp_socket_addr: None,
            use_opus: if authenticate.has_opus() { authenticate.get_opus() } else { false },
            codecs: authenticate.get_celt_versions().to_vec(),
            authenticate,
            udp_socket,
            publisher,
            publisher_disconnect,
            targets,
            last_ping: RwLock::new(Instant::now()),
        }
    }

    pub fn get_target(&self, id: usize) -> Option<Arc<RwLock<VoiceTarget>>> {
        self.targets.get(id).cloned()
    }

    pub async fn send(&self, data: &[u8]) -> Result<(), MumbleError> {
        Ok(self.write.write_err().await?.write_all(data).await?)
    }

    pub fn mute(&mut self, mute: bool) {
        self.mute = mute;
    }

    pub async fn send_message<T: Message>(&self, kind: MessageKind, message: &T) -> Result<(), MumbleError> {
        tracing::trace!(
            "[{}] [{}] send message: {:?}, {:?}",
            self.authenticate.get_username(),
            self.session_id,
            std::any::type_name::<T>(),
            message
        );

        let bytes = message_to_bytes(kind, message)?;

        self.send(bytes.as_ref()).await?;

        crate::metrics::MESSAGES_TOTAL
            .with_label_values(&["tcp", "output", kind.to_string().as_str()])
            .inc();

        crate::metrics::MESSAGES_BYTES
            .with_label_values(&["tcp", "output", kind.to_string().as_str()])
            .inc_by(bytes.len() as u64);

        Ok(())
    }

    pub async fn send_crypt_setup(&self, reset: bool) -> Result<(), MumbleError> {
        if reset {
            {
                self.crypt_state.write_err().await?.reset();
            }
        }

        let crypt_setup = { self.crypt_state.read_err().await?.get_crypt_setup() };

        self.send_message(MessageKind::CryptSetup, &crypt_setup).await
    }

    pub async fn send_my_user_state(&self) -> Result<(), MumbleError> {
        let user_state = self.get_user_state();

        self.send_message(MessageKind::UserState, &user_state).await
    }

    pub async fn sync_client_and_channels(&self, state: &Arc<RwLock<ServerState>>) -> Result<(), MumbleError> {
        let mut stream_write = self.write.write_err().await?;

        {
            let state_read = state.read_err().await?;

            // Send channel states
            for channel in state_read.channels.values() {
                let channel_state = { channel.read_err().await?.get_channel_state() };

                send_message(MessageKind::ChannelState, &channel_state, stream_write.deref_mut()).await?;
            }

            // Send user states
            for client in state_read.clients.values() {
                let user_state = { client.read_err().await?.get_user_state() };

                send_message(MessageKind::UserState, &user_state, stream_write.deref_mut()).await?;
            }
        }

        Ok(())
    }

    pub async fn send_server_sync(&self) -> Result<(), MumbleError> {
        let mut server_sync = ServerSync::default();
        server_sync.set_max_bandwidth(144000);
        server_sync.set_session(self.session_id);
        server_sync.set_welcome_text("SoZ Mumble Server".to_string());

        self.send_message(MessageKind::ServerSync, &server_sync).await
    }

    pub async fn send_server_config(&self) -> Result<(), MumbleError> {
        let mut server_config = ServerConfig::default();
        server_config.set_allow_html(true);
        server_config.set_message_length(512);
        server_config.set_image_message_length(0);

        self.send_message(MessageKind::ServerConfig, &server_config).await
    }

    pub async fn send_voice_packet(&self, packet: &VoicePacket<Clientbound>) -> Result<(), MumbleError> {
        if let Some(addr) = self.udp_socket_addr {
            let mut dest = BytesMut::new();
            self.crypt_state.write_err().await?.encrypt(packet, &mut dest);

            let buf = &dest.freeze()[..];

            self.udp_socket.send_to(buf, addr).await?;

            crate::metrics::MESSAGES_TOTAL
                .with_label_values(&["udp", "output", "VoicePacket"])
                .inc();

            crate::metrics::MESSAGES_BYTES
                .with_label_values(&["udp", "output", "VoicePacket"])
                .inc_by(buf.len() as u64);

            return Ok(());
        }

        let mut data = BytesMut::new();
        encode_voice_packet(packet, &mut data);

        let bytes = data.freeze();

        let mut buffer = BytesMut::new();
        buffer.put_u16(MessageKind::UDPTunnel as u16);
        buffer.put_u32(bytes.len() as u32);
        buffer.put_slice(&bytes);

        {
            let mut stream = self.write.write_err().await?;
            stream.write_all(buffer.as_ref()).await?;
            stream.flush().await?;
        }

        crate::metrics::MESSAGES_TOTAL
            .with_label_values(&["tcp", "output", "VoicePacket"])
            .inc();

        crate::metrics::MESSAGES_BYTES
            .with_label_values(&["tcp", "output", "VoicePacket"])
            .inc_by(buffer.len() as u64);

        Ok(())
    }

    pub fn update(&mut self, state: &UserState) {
        if state.has_mute() {
            self.mute = state.get_mute();
        }

        if state.has_deaf() {
            self.deaf = state.get_deaf();
        }
    }

    pub fn join_channel(&mut self, mut channel_id: u32) -> Option<u32> {
        if channel_id == self.channel_id {
            return None;
        }

        std::mem::swap(&mut self.channel_id, &mut channel_id);

        Some(channel_id)
    }

    pub fn get_user_state(&self) -> UserState {
        let mut user_state = UserState::new();

        user_state.set_user_id(self.session_id);
        user_state.set_channel_id(self.channel_id);
        user_state.set_session(self.session_id);
        user_state.set_name(self.authenticate.get_username().to_string());

        user_state
    }
}
