use crate::error::MumbleError;
use crate::handler::Handler;
use bytes::{BufMut, Bytes, BytesMut};
use protobuf::Message;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub mod mumble;

#[derive(Debug, Clone, Copy)]
pub enum MessageKind {
    Version = 0,
    UDPTunnel = 1,
    Authenticate = 2,
    Ping = 3,
    Reject = 4,
    ServerSync = 5,
    ChannelRemove = 6,
    ChannelState = 7,
    UserRemove = 8,
    UserState = 9,
    BanList = 10,
    TextMessage = 11,
    PermissionDenied = 12,
    ACL = 13,
    QueryUsers = 14,
    CryptSetup = 15,
    ContextActionModify = 16,
    ContextAction = 17,
    UserList = 18,
    VoiceTarget = 19,
    PermissionQuery = 20,
    CodecVersion = 21,
    UserStats = 22,
    RequestBlob = 23,
    ServerConfig = 24,
    SuggestConfig = 25,
}

impl TryFrom<u16> for MessageKind {
    type Error = MumbleError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(MessageKind::Version),
            1 => Ok(MessageKind::UDPTunnel),
            2 => Ok(MessageKind::Authenticate),
            3 => Ok(MessageKind::Ping),
            4 => Ok(MessageKind::Reject),
            5 => Ok(MessageKind::ServerSync),
            6 => Ok(MessageKind::ChannelRemove),
            7 => Ok(MessageKind::ChannelState),
            8 => Ok(MessageKind::UserRemove),
            9 => Ok(MessageKind::UserState),
            10 => Ok(MessageKind::BanList),
            11 => Ok(MessageKind::TextMessage),
            12 => Ok(MessageKind::PermissionDenied),
            13 => Ok(MessageKind::ACL),
            14 => Ok(MessageKind::QueryUsers),
            15 => Ok(MessageKind::CryptSetup),
            16 => Ok(MessageKind::ContextActionModify),
            17 => Ok(MessageKind::ContextAction),
            18 => Ok(MessageKind::UserList),
            19 => Ok(MessageKind::VoiceTarget),
            20 => Ok(MessageKind::PermissionQuery),
            21 => Ok(MessageKind::CodecVersion),
            22 => Ok(MessageKind::UserStats),
            23 => Ok(MessageKind::RequestBlob),
            24 => Ok(MessageKind::ServerConfig),
            25 => Ok(MessageKind::SuggestConfig),
            _ => Err(MumbleError::UnexpectedMessageKind(value)),
        }
    }
}

pub fn message_to_bytes<T: Message>(kind: MessageKind, message: &T) -> Result<Bytes, MumbleError> {
    let bytes = message.write_to_bytes()?;
    let mut buffer = BytesMut::new();
    buffer.put_u16(kind as u16);
    buffer.put_u32(bytes.len() as u32);
    buffer.put_slice(&bytes);

    Ok(buffer.freeze())
}

pub async fn send_message<T: Message, S: AsyncWrite + Unpin>(kind: MessageKind, message: &T, stream: &mut S) -> Result<(), MumbleError> {
    log::trace!("send message: {:?}, {:?}", std::any::type_name::<T>(), message);

    let bytes = message_to_bytes(kind, message)?;
    stream.write_all(bytes.as_ref()).await?;
    stream.flush().await?;

    Ok(())
}

pub async fn expected_message<T: Message + Handler, S: AsyncRead + Unpin>(kind: MessageKind, stream: &mut S) -> Result<T, MumbleError> {
    let message_kind = stream.read_u16().await?;

    if message_kind != kind as u16 {
        let size = stream.read_u32().await?;
        let mut data = vec![0; size as usize];
        stream.read_exact(&mut data).await?;

        return Err(MumbleError::UnexpectedMessageKind(message_kind));
    }

    get_message(stream).await
}

pub async fn get_message<T: Message + Handler, S: AsyncRead + Unpin>(stream: &mut S) -> Result<T, MumbleError> {
    let size = stream.read_u32().await?;
    let mut data = vec![0; size as usize];
    stream.read_exact(&mut data).await?;

    let message = T::parse_from_bytes(data.as_slice())?;

    log::trace!("received message: {:?}, {:?}", std::any::type_name::<T>(), message);

    Ok(message)
}
