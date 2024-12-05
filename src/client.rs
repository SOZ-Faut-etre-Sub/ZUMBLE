use crate::crypt::CryptState;
use crate::error::MumbleError;
use crate::message::ClientMessage;
use crate::proto::mumble::{Authenticate, ServerConfig, ServerSync, UDPTunnel, UserState, Version};
use crate::proto::{expected_message, message_to_bytes, send_message, MessageKind};
use crate::server::constants::MAX_BANDWIDTH;
use crate::state::{ServerState, ServerStateRef};
use crate::target::VoiceTarget;
use crate::voice::{encode_voice_packet, ClientBound, VoicePacket};
use arc_swap::ArcSwapOption;
use bytes::BytesMut;
use crossbeam::atomic::AtomicCell;
use parking_lot::Mutex;
use protobuf::Message;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::io::{AsyncWriteExt, WriteHalf};
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::mpsc::Sender;
use tokio_rustls::server::TlsStream;

pub type ClientRef = Arc<Client>;

type VoiceTargetArray = [Arc<VoiceTarget>; 29];

pub struct Client {
    // pub version: Version,
    name: Arc<String>,
    pub authenticate: Authenticate,
    pub session_id: u32,
    pub channel_id: AtomicU32,
    pub mute: AtomicBool,
    pub deaf: AtomicBool,
    pub write: tokio::sync::Mutex<WriteHalf<TlsStream<TcpStream>>>,
    // pub tokens: Vec<String>,
    pub crypt_state: Mutex<CryptState>,
    pub udp_socket_addr: ArcSwapOption<SocketAddr>,
    // pub use_opus: bool,
    pub codecs: Vec<i32>,
    pub udp_socket: Arc<UdpSocket>,
    pub publisher: Sender<ClientMessage>,
    pub targets: VoiceTargetArray,
    pub last_ping: AtomicCell<Instant>,
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
        _version: Version,
        authenticate: Authenticate,
        session_id: u32,
        channel_id: u32,
        crypt_state: CryptState,
        write: WriteHalf<TlsStream<TcpStream>>,
        udp_socket: Arc<UdpSocket>,
        publisher: Sender<ClientMessage>,
    ) -> Self {
        // let tokens = authenticate.get_tokens().iter().map(|token| token.to_string()).collect();
        let targets: VoiceTargetArray = core::array::from_fn(|_v| Arc::new(VoiceTarget::default()));

        Self {
            // version,
            session_id,
            name: Arc::new(authenticate.get_username().to_string()),
            channel_id: AtomicU32::new(channel_id),
            crypt_state: Mutex::new(crypt_state),
            write: tokio::sync::Mutex::new(write),
            // tokens,
            deaf: AtomicBool::new(false),
            mute: AtomicBool::new(false),
            udp_socket_addr: ArcSwapOption::from(None),
            // use_opus: if authenticate.has_opus() { authenticate.get_opus() } else { false },
            codecs: authenticate.get_celt_versions().to_vec(),
            authenticate,
            udp_socket,
            publisher,
            targets,
            last_ping: AtomicCell::new(Instant::now()),
        }
    }

    /// Gets the current voice target for the specific id
    /// NOTE: Since voice target 0 and 31 can't be used this will automatically reduce the id by
    /// one to reduce the needed storage for voice targets.
    pub fn get_target(&self, id: u8) -> Option<Arc<VoiceTarget>> {
        self.targets.get((id - 1) as usize).cloned()
    }

    pub fn get_name(&self) -> &Arc<String> {
        &self.name
    }

    pub async fn send(&self, data: &[u8]) -> Result<(), MumbleError> {
        let mut writer = self.write.lock().await;
        match writer.write_all(data).await {
            Ok(_bytes) => {
                Ok(())
            }
            Err(e) => Err(e.into()),
        }
    }

    pub fn is_muted(&self) -> bool {
        self.mute.load(Ordering::Relaxed)
    }

    pub fn is_deaf(&self) -> bool {
        self.deaf.load(Ordering::Relaxed)
    }

    pub fn set_mute(&self, mute: bool) {
        self.mute.store(mute, Ordering::Release);
    }

    pub fn set_deaf(&self, deaf: bool) {
        self.deaf.store(deaf, Ordering::Release);
    }

    pub async fn send_message<T: Message>(&self, kind: MessageKind, message: &T) -> Result<(), MumbleError> {
        tracing::trace!(
            "[{}] [{}] send message: {:?}, {:?}",
            self.name,
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

    /// removes the udp socket from the client and returns it to the caller
    pub fn remove_udp_socket(&self) -> Option<Arc<SocketAddr>> {
        // swap the udp socket address for none so we don't keep a copy
        self.udp_socket_addr.swap(None)
    }

    pub async fn send_crypt_setup(&self, reset: bool) -> Result<(), MumbleError> {
        let crypt_setup = {
            let mut crypt = self.crypt_state.lock();
            if reset {
                crypt.reset();
            }

            crypt.get_crypt_setup()
        };

        self.send_message(MessageKind::CryptSetup, &crypt_setup).await
    }

    pub async fn send_my_user_state(&self) -> Result<(), MumbleError> {
        let user_state = self.get_user_state();

        self.send_message(MessageKind::UserState, &user_state).await
    }

    pub async fn sync_client_and_channels(&self, state: &ServerStateRef) -> Result<(), MumbleError> {
        // Send channel states
        let mut iter = state.channels.first_entry_async().await;
        while let Some(channel) = iter {
            let channel_state = { channel.get_channel_state() };

            self.send_message(MessageKind::ChannelState, channel_state.as_ref()).await?;

            iter = channel.next_async().await;
        }

        // send client sates
        let mut iter = state.clients.first_entry_async().await;
        while let Some(client) = iter {
            let user_state = client.get_user_state();

            self.send_message(MessageKind::UserState, &user_state).await?;

            iter = client.next_async().await;
        }

        Ok(())
    }

    pub async fn send_server_sync(&self) -> Result<(), MumbleError> {
        let mut server_sync = ServerSync::default();
        server_sync.set_max_bandwidth(MAX_BANDWIDTH);
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

    pub async fn send_voice_packet(&self, packet: VoicePacket<ClientBound>) -> Result<(), MumbleError> {
        if let Some(addr) = self.udp_socket_addr.load_full() {
            let mut dest = BytesMut::new();

            {
                self.crypt_state.lock().encrypt(&packet, &mut dest);
            }

            let buf = &dest.freeze()[..];

            self.udp_socket.send_to(buf, addr.as_ref()).await?;

            crate::metrics::MESSAGES_TOTAL
                .with_label_values(&["udp", "output", "VoicePacket"])
                .inc();

            crate::metrics::MESSAGES_BYTES
                .with_label_values(&["udp", "output", "VoicePacket"])
                .inc_by(buf.len() as u64);

            return Ok(());
        }

        let mut data = BytesMut::new();
        encode_voice_packet(&packet, &mut data);
        let bytes = data.freeze();

        let mut tunnel_message = UDPTunnel::default();
        tunnel_message.set_packet(bytes.to_vec());

        self.send_message(MessageKind::UDPTunnel, &tunnel_message).await
    }

    pub fn update(&self, state: &UserState) {
        if state.has_mute() {
            self.set_mute(state.get_mute());
        }

        if state.has_deaf() {
            self.set_deaf(state.get_deaf());
        }
    }

    pub fn join_channel(&self, channel_id: u32) -> Option<u32> {
        let current_channel = self.channel_id.load(Ordering::Relaxed);

        if channel_id == current_channel {
            return None;
        }

        self.channel_id.store(channel_id, Ordering::Relaxed);

        Some(current_channel)
    }

    pub fn get_user_state(&self) -> UserState {
        let mut user_state = UserState::new();

        user_state.set_user_id(self.session_id);
        user_state.set_channel_id(self.channel_id.load(Ordering::Relaxed));
        user_state.set_session(self.session_id);
        user_state.set_name(self.get_name().as_ref().clone());

        user_state
    }
}
