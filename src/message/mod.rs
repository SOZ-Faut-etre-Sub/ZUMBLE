use crate::proto::MessageKind;
use crate::voice::{ClientBound, VoicePacket};
use bytes::Bytes;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub enum ClientMessage {
    RouteVoicePacket(VoicePacket<ClientBound>),
    SendVoicePacket(Arc<VoicePacket<ClientBound>>),
    SendMessage { kind: MessageKind, payload: Bytes },
    Disconnect,
}
