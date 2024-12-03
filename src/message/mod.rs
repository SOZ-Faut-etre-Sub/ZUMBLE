use crate::proto::MessageKind;
use crate::voice::{ClientBound, VoicePacket};
use bytes::Bytes;

#[derive(Debug, Clone)]
pub enum ClientMessage {
    RouteVoicePacket(VoicePacket<ClientBound>),
    SendVoicePacket(VoicePacket<ClientBound>),
    SendMessage { kind: MessageKind, payload: Bytes },
    Disconnect,
}
