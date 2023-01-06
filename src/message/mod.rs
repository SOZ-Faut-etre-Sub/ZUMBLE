use crate::proto::MessageKind;
use crate::voice::{Clientbound, Serverbound, VoicePacket};
use bytes::Bytes;

#[derive(Debug, Clone)]
pub enum ClientMessage {
    RouteVoicePacket(VoicePacket<Clientbound>),
    SendVoicePacket(VoicePacket<Clientbound>),
    TryVoicePacket(VoicePacket<Serverbound>),
    SendMessage { kind: MessageKind, payload: Bytes },
    UpdateMut(bool),
    Disconnect,
}
