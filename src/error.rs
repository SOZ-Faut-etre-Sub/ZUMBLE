use crate::message::ClientMessage;
use thiserror::Error;

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
    #[error("force disconnecting client")]
    ForceDisconnect,
    #[error("send message error: {0}")]
    SendError(#[from] tokio::sync::mpsc::error::SendTimeoutError<ClientMessage>),
    #[error("timeout error")]
    Timeout,
}

impl actix_web::error::ResponseError for MumbleError {}

#[derive(Error, Debug)]
pub enum DecryptError {
    #[error("tokio io error: {0}")]
    Io(#[from] tokio::io::Error),
    #[error("unexpected eof")]
    Eof,
    #[error("repeat error")]
    Repeat,
    #[error("late error")]
    Late,
    #[error("mac error")]
    Mac,
}
