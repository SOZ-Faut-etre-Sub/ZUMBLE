//! Voice channel packets and codecs

use crate::error::DecryptError;
use byteorder::ReadBytesExt;
use bytes::Buf;
use bytes::BufMut;
use bytes::Bytes;
use bytes::BytesMut;
use std::fmt::Debug;
use std::io;
use std::io::{Cursor, Read};
use std::marker::PhantomData;

use super::varint::BufMutExt;
use super::varint::ReadExt;

/// A packet transmitted via Mumble's voice channel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VoicePacket<Dst: VoicePacketDst> {
    /// Ping packets contain opaque timestamp-like values which should simply be echoed back.
    Ping {
        /// Opaque timestamp-like value.
        /// Unless this is the echo, no assumptions about it should be made.
        timestamp: u64,
    },
    /// Packet containing audio data.
    Audio {
        /// Destination. Required due to encoding differences depending on packet flow direction.
        _dst: PhantomData<Dst>,
        /// The target.
        ///
        /// Only values 0-31 are valid (when serialized, this field is 5-bits long).
        target: u8,
        /// Session ID. Absent when packet is [Serverbound].
        session_id: Dst::SessionId,
        /// Sequence number of the first audio frame in this packet.
        ///
        /// Packets may contain multiple frames, so this may increase by more than one per packet.
        seq_num: u64,
        /// The actual audio data
        payload: VoicePacketPayload,
        /// Positional audio information.
        ///
        /// Usually `[f32; 3]` but may contain additional or different data if all clients
        /// receiving this packet can deal with such values (e.g. games with builtin Mumble
        /// client may use this field to transmit additional data to other game clients).
        position_info: Option<Bytes>,
    },
}

/// Audio data payload of [VoicePacket]s.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum VoicePacketPayload {
    /// CELT Alpha (0.7.0) encoded audio frames.
    CeltAlpha(Vec<Bytes>),
    /// CELT Beta (0.11.0) encoded audio frames.
    CeltBeta(Vec<Bytes>),
    /// Speex encoded audio frames.
    Speex(Vec<Bytes>),
    /// Opus encoded audio frame with end-of-transmission bit.
    Opus(Bytes, bool),
}

/// Zero-sized struct indicating server-bound packet direction.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ServerBound;
/// Zero-sized struct indicating client-bound packet direction.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClientBound;

/// Sealed trait for indicating voice packet direction.
///
/// The only two implementations are [Serverbound] and [Clientbound].
pub trait VoicePacketDst: Default + PartialEq {
    /// Type of [VoicePacket::Audio::session_id](enum.VoicePacket.html#variant.Audio.field.session_id).
    type SessionId: Debug + Clone + PartialEq;
    /// Reads session id of packets traveling in this direction.
    fn read_session_id<T: Read + Sized>(buf: &mut T) -> Result<Self::SessionId, io::Error>;
    /// Writes session id to packets traveling in this direction.
    fn write_session_id(buf: &mut BytesMut, session_id: &Self::SessionId);
}

impl VoicePacketDst for ServerBound {
    type SessionId = ();

    fn read_session_id<T: Read + Sized>(_buf: &mut T) -> Result<Self::SessionId, io::Error> {
        Ok(())
    }

    fn write_session_id(_buf: &mut BytesMut, _session_id: &Self::SessionId) {}
}

impl VoicePacketDst for ClientBound {
    type SessionId = u32;

    fn read_session_id<T: Read + Sized>(buf: &mut T) -> Result<Self::SessionId, io::Error> {
        Ok(buf.read_varint()? as u32)
    }

    fn write_session_id(buf: &mut BytesMut, session_id: &Self::SessionId) {
        buf.put_varint(u64::from(*session_id))
    }
}

impl VoicePacket<ServerBound> {
    pub fn into_client_bound(self, session_id: u32) -> VoicePacket<ClientBound> {
        match self {
            VoicePacket::Ping { timestamp } => VoicePacket::Ping { timestamp },
            VoicePacket::Audio {
                target,
                seq_num,
                payload,
                position_info,
                ..
            } => VoicePacket::Audio {
                _dst: PhantomData,
                target,
                session_id,
                seq_num,
                payload,
                position_info,
            },
        }
    }
}

pub fn decode_voice_packet<DecodeDst: VoicePacketDst>(buf_mut: &mut BytesMut) -> Result<VoicePacket<DecodeDst>, DecryptError> {
    let mut buf = Cursor::new(&buf_mut);
    let header = buf.read_u8()?;
    let kind = header >> 5;
    let target = header & 0b11111;
    let result = if kind == 1 {
        let timestamp = buf.read_varint()?;
        buf_mut.advance(buf_mut.len());
        VoicePacket::Ping { timestamp }
    } else {
        let session_id = DecodeDst::read_session_id(&mut buf)?;
        let seq_num = buf.read_varint()?;
        let payload = match kind {
            0 | 2 | 3 => {
                let mut frames = Vec::new();
                let position = buf.position();
                buf_mut.advance(position as usize);
                loop {
                    if buf_mut.is_empty() {
                        return Err(DecryptError::Eof);
                    }
                    let header = buf_mut[0];
                    buf_mut.advance(1);

                    let len = (header & !0x80) as usize;
                    if buf_mut.len() < len {
                        return Err(DecryptError::Eof);
                    }
                    frames.push(buf_mut.split_to(len).freeze());
                    if header & 0x80 != 0x80 {
                        break;
                    }
                }
                match kind {
                    0 => VoicePacketPayload::CeltAlpha(frames),
                    2 => VoicePacketPayload::Speex(frames),
                    3 => VoicePacketPayload::CeltBeta(frames),
                    _ => panic!(),
                }
            }
            4 => {
                let header = buf.read_varint()?;
                let position = buf.position();
                buf_mut.advance(position as usize);
                let termination_bit = header & 0x2000 == 0x2000;
                let len = (header & !0x2000) as usize;
                if buf_mut.len() < len {
                    return Err(DecryptError::Eof);
                }
                let frame = buf_mut.split_to(len).freeze();
                VoicePacketPayload::Opus(frame, termination_bit)
            }
            _ => {
                return Err(DecryptError::Io(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "unknown voice packet type",
                )));
            }
        };
        let position_info = if buf_mut.is_empty() { None } else { Some(buf_mut.split().freeze()) };
        VoicePacket::Audio {
            _dst: PhantomData,
            target,
            session_id,
            seq_num,
            payload,
            position_info,
        }
    };
    Ok(result)
}

pub fn encode_voice_packet<EncodeDst: VoicePacketDst>(item: &VoicePacket<EncodeDst>, dst: &mut BytesMut) {
    match item {
        VoicePacket::Ping { timestamp } => {
            dst.reserve(11);
            dst.put_u8(0x20);
            dst.put_varint(*timestamp);
        }
        VoicePacket::Audio {
            _dst,
            target,
            session_id,
            seq_num,
            payload,
            position_info,
        } => {
            let kind = match payload {
                VoicePacketPayload::CeltAlpha(_) => 0,
                VoicePacketPayload::Speex(_) => 2,
                VoicePacketPayload::CeltBeta(_) => 3,
                VoicePacketPayload::Opus(_, _) => 4,
            };
            dst.reserve(1 /*header*/ + 10 /*session_id*/ + 10 /*seq_num*/);
            dst.put_u8(kind << 5 | target & 0b11111);
            EncodeDst::write_session_id(dst, session_id);
            dst.put_varint(*seq_num);
            match payload {
                VoicePacketPayload::CeltAlpha(frames) | VoicePacketPayload::Speex(frames) | VoicePacketPayload::CeltBeta(frames) => {
                    dst.reserve(frames.iter().map(|frame| 1 + frame.len()).sum());
                    let mut iter = frames.iter().peekable();
                    while let Some(frame) = iter.next() {
                        let continuation = iter.peek().map(|_| 0x80).unwrap_or(0);
                        dst.put_u8(continuation | (frame.len() as u8));
                        dst.put(frame.as_ref());
                    }
                }
                VoicePacketPayload::Opus(frame, termination_bit) => {
                    dst.reserve(10 + frame.len());
                    let term_bit = if *termination_bit { 0x2000 } else { 0 };
                    dst.put_varint(term_bit | (frame.len() as u64));
                    dst.put(frame.as_ref());
                }
            };
            if let Some(bytes) = position_info {
                dst.extend_from_slice(bytes);
            }
        }
    }
}
