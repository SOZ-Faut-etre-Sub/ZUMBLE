mod authenticate;
mod channel_state;
mod crypt_setup;
mod permission_query;
mod ping;
mod user_state;
mod version;
mod voice_packet;
mod voice_target;

use crate::client::ClientRef;
use crate::error::MumbleError;
use crate::message::ClientMessage;
use crate::proto::mumble;
use crate::proto::MessageKind;
use crate::state::ServerStateRef;
use crate::voice::{decode_voice_packet, ServerBound};
use anyhow::Context;
use async_trait::async_trait;
use bytes::BytesMut;
use protobuf::Message;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::sync::mpsc::Receiver;

#[async_trait]
pub trait Handler {
    async fn handle(&self, state: ServerStateRef, client: ClientRef) -> Result<(), MumbleError>;
}

pub struct MessageHandler;

impl MessageHandler {
    async fn try_handle<T: Message + Handler>(buf: &[u8], state: ServerStateRef, client: ClientRef) -> Result<(), MumbleError> {
        let message = T::parse_from_bytes(buf)?;

        let (username, client_id) = { (client.authenticate.get_username().to_string(), client.session_id) };

        tracing::trace!(
            "[{}] [{}] handle message: {:?}, {:?}",
            username,
            client_id,
            std::any::type_name::<T>(),
            message
        );

        message.handle(state, client).await?;
        Ok(())
    }

    pub async fn handle<S: AsyncRead + Unpin>(
        stream: &mut S,
        consumer: &mut Receiver<ClientMessage>,
        state: ServerStateRef,
        client: ClientRef,
    ) -> Result<(), anyhow::Error> {
        tokio::select! {
            kind_read = stream.read_u16() => {
                let kind = kind_read?;
                let size = stream.read_u32().await?;
                let mut buf = vec![0; size as usize];
                stream.read_exact(&mut buf).await?;

                let message_kind = MessageKind::try_from(kind)?;

                crate::metrics::MESSAGES_TOTAL.with_label_values(&["tcp", "input", message_kind.to_string().as_str()]).inc();
                crate::metrics::MESSAGES_BYTES.with_label_values(&["tcp", "input", message_kind.to_string().as_str()]).inc_by(buf.len() as u64);

                match message_kind {
                    MessageKind::Version => Self::try_handle::<mumble::Version>(&buf, state, client).await.context("kind: Version"),
                    MessageKind::UDPTunnel => {
                        let mut bytes = BytesMut::from(buf.as_slice());

                        let voice_packet = match decode_voice_packet::<ServerBound>(&mut bytes) {
                            Ok(voice_packet) => voice_packet,
                            Err(e) => {
                                tracing::error!("error decoding voice packet: {}", e);

                                return Ok(());
                            }
                        };

                        let output_voice_packet = { voice_packet.into_client_bound(client.session_id) };

                        output_voice_packet.handle(state, client).await.context("kind: UDPTunnel")
                    }
                    MessageKind::Authenticate => Self::try_handle::<mumble::Authenticate>(&buf, state, client).await.context("kind: Authenticate"),
                    MessageKind::Ping => Self::try_handle::<mumble::Ping>(&buf, state, client).await.context("kind: Ping =>"),
                    MessageKind::ChannelState => Self::try_handle::<mumble::ChannelState>(&buf, state, client).await.context("kind: ChannelState"),
                    MessageKind::CryptSetup => Self::try_handle::<mumble::CryptSetup>(&buf, state, client).await.context("kind: CryptSetup"),
                    MessageKind::PermissionQuery => Self::try_handle::<mumble::PermissionQuery>(&buf, state, client).await.context("kind: PermissionQuery"),
                    MessageKind::UserState => Self::try_handle::<mumble::UserState>(&buf, state, client).await.context("kind: UserState"),
                    MessageKind::VoiceTarget => Self::try_handle::<mumble::VoiceTarget>(&buf, state, client).await.context("kind: VoiceTarget"),
                    _ => {
                        tracing::warn!("unsupported message kind: {:?}", message_kind);

                        Ok(())
                    }
                }
            },
            consume = consumer.recv() => {
                match consume {
                    Some(ClientMessage::RouteVoicePacket(packet)) => {
                        packet.handle(state, client).await.context("handle voice packet")
                    },
                    Some(ClientMessage::SendVoicePacket(packet)) => {
                        client.send_voice_packet(packet).await.context("send voice packet")
                    },
                    Some(ClientMessage::SendMessage { kind, payload }) => {
                        client.send(payload.as_ref()).await.context(format!("send message of type: {}", kind))
                    },
                    Some(ClientMessage::Disconnect) => {
                        Err(MumbleError::ForceDisconnect).context("force disconnect")
                    },
                    _ => {
                        Ok(())
                    }
                }
            },
        }
    }
}
