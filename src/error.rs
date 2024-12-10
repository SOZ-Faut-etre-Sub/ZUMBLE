use thiserror::Error;

use crate::message::ClientMessage;

#[derive(Error, Debug)]
pub enum MumbleError {
    #[error("unexpected message kind: {0}")]
    UnexpectedMessageKind(u16),
    #[error("tokio io error: {0}")]
    Io(#[from] tokio::io::Error),
    #[error("protobuf error: {0}")]
    Parse(#[from] protobuf::ProtobufError),
    #[error("voice decrypt error: {0}")]
    Decrypt(#[from] DecryptError),
    #[error("Client Forcefully disconnected from server")]
    ForceDisconnect,
    #[error("send message error: {0}")]
    SendError(#[from] tokio::sync::mpsc::error::SendTimeoutError<ClientMessage>),
    #[error("invalid voice target id")]
    InvalidVoiceTarget,
    #[error("channel doesn't exist")]
    ChannelDoesntExist,
    #[error("voice packet took to long to send, discarding")]
    PacketDiscarded
}

impl actix_web::error::ResponseError for MumbleError {}

#[derive(Error, Debug)]
pub enum DecryptError {
    #[error("tokio io error: {0}")]
    Io(#[from] tokio::io::Error),
    #[error("unexpected eof")]
    Eof,
    #[error("Client sent a repeat packet, discarding")]
    Repeat,
    #[error("Client sent a packet that was received late, discarding")]
    Late,
    #[error("mac error")]
    Mac,
}
