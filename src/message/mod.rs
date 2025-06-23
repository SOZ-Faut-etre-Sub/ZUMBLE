use bytes::Bytes;

use crate::{
    proto::MessageKind,
    voice::{ClientBound, VoicePacket},
};

#[derive(Debug, Clone)]
pub enum ClientMessage {
    RouteVoicePacket(VoicePacket<ClientBound>),
    SendVoicePacket(VoicePacket<ClientBound>),
    SendMessage { kind: MessageKind, payload: Bytes },
}
