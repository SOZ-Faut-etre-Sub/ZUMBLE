use crate::proto::MessageKind;
use crate::voice::{Clientbound, VoicePacket};
use bytes::Bytes;

#[derive(Debug, Clone)]
pub enum ClientMessage {
    RouteVoicePacket(VoicePacket<Clientbound>),
    SendVoicePacket(VoicePacket<Clientbound>),
    SendMessage { kind: MessageKind, payload: Bytes },
    Disconnect,
}
