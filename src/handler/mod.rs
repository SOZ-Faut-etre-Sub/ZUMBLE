mod authenticate;
mod channel_state;
mod crypt_setup;
mod permission_query;
mod ping;
mod user_state;
mod version;
mod voice_packet;
mod voice_target;

use crate::client::Client;
use crate::error::MumbleError;
use crate::proto::mumble;
use crate::proto::MessageKind;
use crate::voice::{decode_voice_packet, Clientbound, Serverbound, VoicePacket};
use crate::ServerState;
use async_trait::async_trait;
use bytes::BytesMut;
use protobuf::Message;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::sync::mpsc::Receiver;
use tokio::sync::RwLock;

#[async_trait]
pub trait Handler {
    async fn handle(&self, state: Arc<RwLock<ServerState>>, client: Arc<RwLock<Client>>) -> Result<(), MumbleError>;
}

pub struct MessageHandler;

impl MessageHandler {
    async fn try_handle<T: Message + Handler>(
        buf: &[u8],
        state: Arc<RwLock<ServerState>>,
        client: Arc<RwLock<Client>>,
    ) -> Result<(), MumbleError> {
        let message = T::parse_from_bytes(buf)?;

        let (username, client_id) = {
            let client = client.read().await;
            (client.authenticate.get_username().to_string(), client.session_id)
        };

        log::trace!(
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
        consumer: &mut Receiver<VoicePacket<Clientbound>>,
        state: Arc<RwLock<ServerState>>,
        client: Arc<RwLock<Client>>,
    ) -> Result<(), MumbleError> {
        tokio::select! {
            kind_read = stream.read_u16() => {
                let kind = kind_read?;
                let size = stream.read_u32().await?;
                let mut buf = vec![0; size as usize];
                stream.read_exact(&mut buf).await?;

                let message_kind = MessageKind::try_from(kind)?;

                match message_kind {
                    MessageKind::Version => return Self::try_handle::<mumble::Version>(&buf, state, client).await,
                    MessageKind::UDPTunnel => {
                        let mut bytes = BytesMut::from(buf.as_slice());

                        let voice_packet = match decode_voice_packet::<Serverbound>(&mut bytes) {
                            Ok(voice_packet) => voice_packet,
                            Err(e) => {
                                log::error!("error decoding voice packet: {}", e);

                                return Ok(());
                            }
                        };

                        let output_voice_packet = { voice_packet.to_client_bound(client.read().await.session_id) };

                        return output_voice_packet.handle(state, client).await
                    }
                    MessageKind::Authenticate => return Self::try_handle::<mumble::Authenticate>(&buf, state, client).await,
                    MessageKind::Ping => return Self::try_handle::<mumble::Ping>(&buf, state, client).await,
                    MessageKind::ChannelState => return Self::try_handle::<mumble::ChannelState>(&buf, state, client).await,
                    MessageKind::CryptSetup => return Self::try_handle::<mumble::CryptSetup>(&buf, state, client).await,
                    MessageKind::PermissionQuery => return Self::try_handle::<mumble::PermissionQuery>(&buf, state, client).await,
                    MessageKind::UserState => return Self::try_handle::<mumble::UserState>(&buf, state, client).await,
                    MessageKind::VoiceTarget => return Self::try_handle::<mumble::VoiceTarget>(&buf, state, client).await,
                    _ => {
                        log::warn!("unsupported message kind: {:?}", message_kind);

                        return Ok(());
                    }
                }
            },
            packet = consumer.recv() => {
                if let Some(packet) = packet {
                    return packet.handle(state, client).await
                } else {
                    return Ok(());
                }
            }
        }
    }
}
