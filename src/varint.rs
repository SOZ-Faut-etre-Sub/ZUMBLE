//! Extension traits for Mumble's varint format.

use byteorder::ReadBytesExt;
use byteorder::WriteBytesExt;
use bytes::BufMut;
use std::io;

/// Extension trait for reading varint values.
pub trait ReadExt: io::Read {
    /// Reads a 64-bit varint.
    fn read_varint(&mut self) -> io::Result<u64>;
}

/// Extension trait for writing varint values.
pub trait WriteExt: io::Write {
    /// Writes a 64-bit varint.
    fn write_varint(&mut self, val: u64) -> io::Result<()>;
}

/// Extension trait for writing varint values to [BufMut]s.
pub trait BufMutExt: BufMut {
    /// Writes a 64-bit varint.
    fn put_varint(&mut self, val: u64);
}

impl<T: io::Read> ReadExt for T {
    fn read_varint(&mut self) -> io::Result<u64> {
        let b0 = self.read_u8()?;
        if b0 & 0b1111_1100 == 0b1111_1000 {
            return Ok(!self.read_varint()?);
        }
        if b0 & 0b1111_1100 == 0b1111_1100 {
            return Ok(!u64::from(b0 & 0x03));
        }
        if (b0 & 0b1000_0000) == 0 {
            return Ok(u64::from(b0 & 0b0111_1111));
        }
        let b1 = self.read_u8()?;
        if (b0 & 0b0100_0000) == 0 {
            return Ok(u64::from(b0 & 0b0011_1111) << 8 | u64::from(b1));
        }
        let b2 = self.read_u8()?;
        if (b0 & 0b0010_0000) == 0 {
            return Ok(u64::from(b0 & 0b0001_1111) << 16 | u64::from(b1) << 8 | u64::from(b2));
        }
        let b3 = self.read_u8()?;
        if (b0 & 0b0001_0000) == 0 {
            return Ok(u64::from(b0 & 0x0F) << 24 | u64::from(b1) << 16 | u64::from(b2) << 8 | u64::from(b3));
        }
        let b4 = self.read_u8()?;
        if (b0 & 0b0000_0100) == 0 {
            return Ok(u64::from(b1) << 24 | u64::from(b2) << 16 | u64::from(b3) << 8 | u64::from(b4));
        }
        let b5 = self.read_u8()?;
        let b6 = self.read_u8()?;
        let b7 = self.read_u8()?;
        let b8 = self.read_u8()?;
        Ok(u64::from(b1) << 56
            | u64::from(b2) << 48
            | u64::from(b3) << 40
            | u64::from(b4) << 32
            | u64::from(b5) << 24
            | u64::from(b6) << 16
            | u64::from(b7) << 8
            | u64::from(b8))
    }
}

impl<T: io::Write> WriteExt for T {
    fn write_varint(&mut self, value: u64) -> io::Result<()> {
        if value & 0xffff_ffff_ffff_fffc == 0xffff_ffff_ffff_fffc {
            return self.write_u8(0b1111_1100 | (!value as u8));
        }
        if value & 0x8000_0000_0000_0000 == 0x8000_0000_0000_0000 {
            self.write_u8(0b1111_1000)?;
            return self.write_varint(!value);
        }

        if value > 0xffff_ffff {
            self.write_u8(0b1111_0100)?;
            self.write_u8((value >> 56) as u8)?;
            self.write_u8((value >> 48) as u8)?;
            self.write_u8((value >> 40) as u8)?;
            self.write_u8((value >> 32) as u8)?;
            self.write_u8((value >> 24) as u8)?;
            self.write_u8((value >> 16) as u8)?;
            self.write_u8((value >> 8) as u8)?;
            return self.write_u8(value as u8);
        }

        if value > 0x0fff_ffff {
            self.write_u8(0b1111_0000)?;
            self.write_u8((value >> 24) as u8)?;
            self.write_u8((value >> 16) as u8)?;
            self.write_u8((value >> 8) as u8)?;
            return self.write_u8(value as u8);
        }

        if value > 0x001f_ffff {
            self.write_u8(0b1110_0000 | (value >> 24) as u8)?;
            self.write_u8((value >> 16) as u8)?;
            self.write_u8((value >> 8) as u8)?;
            return self.write_u8(value as u8);
        }

        if value > 0x0000_3fff {
            self.write_u8(0b1100_0000 | (value >> 16) as u8)?;
            self.write_u8((value >> 8) as u8)?;
            return self.write_u8(value as u8);
        }

        if value > 0x0000_007f {
            self.write_u8(0b1000_0000 | (value >> 8) as u8)?;
            return self.write_u8(value as u8);
        }

        self.write_u8(value as u8)
    }
}

impl<T: BufMut> BufMutExt for T {
    fn put_varint(&mut self, val: u64) {
        self.writer().write_varint(val).expect("BufMut::writer never errors");
    }
}
