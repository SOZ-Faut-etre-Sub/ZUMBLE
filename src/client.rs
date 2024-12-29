use crate::crypt::CryptState;
use crate::error::MumbleError;
use crate::message::ClientMessage;
use crate::proto::mumble::{Authenticate, ServerConfig, ServerSync, UDPTunnel, UserState, Version};
use crate::proto::{expected_message, message_to_bytes, send_message, MessageKind};
use crate::server::constants::MAX_BANDWIDTH_IN_BITS;
use crate::state::ServerStateRef;
use crate::target::VoiceTarget;
use crate::voice::{encode_voice_packet, ClientBound, VoicePacket};
use arc_swap::ArcSwapOption;
use bytes::BytesMut;
use crossbeam::atomic::AtomicCell;
use protobuf::reflect::ProtobufValue;
use protobuf::Message;
use scc::ebr::Guard;
use std::fmt::Display;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};
use tokio::io::{AsyncWriteExt, WriteHalf};
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::mpsc::Sender;
use tokio::sync::Mutex;
use tokio::time::timeout;
use tokio_rustls::server::TlsStream;
use tokio_util::sync::CancellationToken;

pub type ClientArc = Arc<Client>;
pub type WeakClient = Weak<Client>;

type VoiceTargetArray = [Arc<VoiceTarget>; 29];

pub struct Client {
    // pub version: Version,
    name: Arc<String>,
    pub log_name: Arc<String>,
    pub authenticate: Authenticate,
    pub session_id: u32,
    pub channel_id: AtomicU32,
    pub mute: AtomicBool,
    pub deaf: AtomicBool,
    pub write: Mutex<WriteHalf<TlsStream<TcpStream>>>,
    // pub tokens: Vec<String>,
    pub crypt_state: Mutex<CryptState>,
    pub udp_socket_addr: ArcSwapOption<SocketAddr>,
    // Token used to cancel any tasks related to this client, i.e. tcp/udp loops
    pub cancel_token: CancellationToken,
    pub udp_socket: Arc<UdpSocket>,
    pub publisher: Sender<ClientMessage>,
    pub targets: VoiceTargetArray,
    pub last_tcp_ping: AtomicCell<Instant>,
    pub last_udp_ping: AtomicCell<Instant>,
    // the amount of bad tcp messages the client has sent, after 20 the client will be dropped automatically
    pub bad_tcp_count: AtomicU32,
}

impl Display for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.log_name)
    }
}

impl Client {
    /// Handles the initial client hand shake, this can fail if the client sends a bunch of UDPTunnel packets (by default 10)
    /// or if the client fails to respond to the request for their version/auth packets within the specified time (by default 1 second)
    pub async fn init(
        stream: &mut TlsStream<TcpStream>,
        server_version: Version,
    ) -> Result<(Version, Authenticate, CryptState), MumbleError> {
        let version: Version = match timeout(Duration::from_secs(1), expected_message(MessageKind::Version, stream)).await {
            Ok(version) => version?,
            Err(e) => {
                return Err(MumbleError::ClientInitFailed(e));
            }
        };

        // Send version
        send_message(MessageKind::Version, &server_version, stream).await?;

        // Get authenticate
        let authenticate: Authenticate = match timeout(Duration::from_secs(1), expected_message(MessageKind::Authenticate, stream)).await {
            Ok(auth) => auth?,
            Err(e) => return Err(MumbleError::ClientInitFailed(e)),
        };

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
    ) -> Arc<Self> {
        // let tokens = authenticate.get_tokens().iter().map(|token| token.to_string()).collect();
        let targets: VoiceTargetArray = core::array::from_fn(|_v| Arc::new(VoiceTarget::default()));

        Arc::new(Self {
            // version,
            session_id,
            log_name: Arc::new(format!("{} [session id: {}]", authenticate.get_username(), session_id)),

            name: Arc::new(authenticate.get_username().to_string()),
            channel_id: AtomicU32::new(channel_id),
            crypt_state: Mutex::new(crypt_state),
            write: Mutex::new(write),
            // tokens,
            deaf: AtomicBool::new(false),
            mute: AtomicBool::new(false),
            udp_socket_addr: ArcSwapOption::from(None),
            cancel_token: CancellationToken::new(),
            authenticate,
            udp_socket,
            publisher,
            targets,
            last_tcp_ping: AtomicCell::new(Instant::now()),
            last_udp_ping: AtomicCell::new(Instant::now()),
            bad_tcp_count: AtomicU32::new(0),
        })
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
        // if our cancel token gets called mid write we should abort out of our write with an error
        tokio::select! {
             _ = self.cancel_token.cancelled() => {
                 return Err(MumbleError::WritterShutDown);
             }
             mut writer_lock = self.write.lock() => {
                 tokio::select! {
                     _ = self.cancel_token.cancelled() => {
                         return Err(MumbleError::WritterShutDown);
                     }
                     res = writer_lock.write_all(data) => {
                         match res {
                            Ok(_bytes) => Ok(()),
                            Err(e) => Err(e.into()),

                         }
                     }

                 }
             }
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

    // TODO: If https://github.com/citizenfx/fivem/pull/2990 gets merged this should only send back
    // the server nonce for unless the clients request a resync
    pub async fn send_crypt_setup(&self, reset: bool) -> Result<(), MumbleError> {
        let crypt_setup = {
            let mut crypt = self.crypt_state.lock().await;
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
        // will contain Weak counts to the channels/clients to get the initial state from
        let mut weak_channels = Vec::new();
        let mut weak_clients = Vec::new();

        // iterate through the channels sync so we don't hold any locks longer than we need to
        {
            let guard = Guard::new();
            for (_k, channel) in state.channels.iter(&guard) {
                // give the channel stat a weak reference to the current channel, as the channel might
                // not exist when this is actually called
                weak_channels.push(Arc::downgrade(&channel));
            }
        }

        // same for clients
        {
            let guard = Guard::new();
            for (_k, client) in state.clients.iter(&guard) {
                weak_clients.push(Arc::downgrade(&client))
            }
        }

        for channel in weak_channels {
            if let Some(channel) = channel.upgrade() {
                self.send_message(MessageKind::ChannelState, channel.get_channel_state().as_ref())
                    .await?;
            }
        }

        for user in weak_clients {
            if let Some(user) = user.upgrade() {
                self.send_message(MessageKind::UserState, &user.get_user_state()).await?;
            }
        }

        Ok(())
    }

    pub async fn send_server_sync(&self) -> Result<(), MumbleError> {
        let mut server_sync = ServerSync::default();
        server_sync.set_max_bandwidth(MAX_BANDWIDTH_IN_BITS);
        server_sync.set_session(self.session_id);
        server_sync.set_welcome_text("SoZ Mumble Server".to_string());

        self.send_message(MessageKind::ServerSync, &server_sync).await
    }

    pub async fn send_server_config(&self) -> Result<(), MumbleError> {
        let mut server_config = ServerConfig::default();
        server_config.set_allow_html(false);
        server_config.set_message_length(0);
        server_config.set_image_message_length(0);

        self.send_message(MessageKind::ServerConfig, &server_config).await
    }

    pub async fn send_voice_packet(&self, packet: VoicePacket<ClientBound>) -> Result<(), MumbleError> {
        if let Some(addr) = self.udp_socket_addr.load_full() {
            let mut dest = BytesMut::new();

            {
                self.crypt_state.lock().await.encrypt(&packet, &mut dest);
            }

            let buf = &dest.freeze()[..];

            match timeout(Duration::from_millis(250), self.udp_socket.send_to(buf, addr.as_ref())).await {
                Ok(Ok(_)) => Ok(()),
                Ok(Err(e)) => Err(MumbleError::Io(e)),
                Err(_) => Err(MumbleError::PacketDiscarded),
            }?;

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
