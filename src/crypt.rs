use crate::error::DecryptError;
use crate::proto::mumble::CryptSetup;
use crate::voice::{decode_voice_packet, encode_voice_packet, VoicePacket, VoicePacketDst};
use actix_web::web::BytesMut;
use aes::cipher::generic_array::GenericArray;
use aes::cipher::{BlockDecrypt, BlockEncrypt, KeyInit};
use aes::Aes128;
use ring::rand::{SecureRandom, SystemRandom};
use std::time::Instant;

lazy_static! {
    static ref SYSTEM_RANDOM: SystemRandom = SystemRandom::new();
}

const KEY_SIZE: usize = 16;
const BLOCK_SIZE: usize = std::mem::size_of::<u128>();

pub struct CryptState {
    pub key: [u8; KEY_SIZE],
    // internally as native endianness, externally as little endian and during ocb_* as big endian
    encrypt_nonce: u128,
    decrypt_nonce: u128,
    decrypt_history: [u8; 0x100],
    aes: Aes128,

    pub good: u32,
    pub late: u32,
    pub lost: u32,
    pub resync: u32,
    pub last_good: Instant,
}

impl Default for CryptState {
    fn default() -> Self {
        let mut key = [0u8; KEY_SIZE];
        SYSTEM_RANDOM.fill(&mut key).expect("Failed to generate random key");

        Self {
            aes: Aes128::new(GenericArray::from_slice(&key)),
            key,
            encrypt_nonce: 0,
            decrypt_nonce: 1 << 127,
            decrypt_history: [0; 0x100],

            good: 0,
            late: 0,
            lost: 0,
            resync: 0,
            last_good: Instant::now(),
        }
    }
}

impl CryptState {
    pub fn reset(&mut self) {
        self.encrypt_nonce = 0;
        self.decrypt_nonce = 1 << 127;
        self.decrypt_history = [0; 0x100];
        self.good = 0;
        self.late = 0;
        self.lost = 0;
        self.resync = 0;
        self.last_good = Instant::now();
    }

    /// Returns the nonce used for encrypting.
    pub fn get_encrypt_nonce(&self) -> [u8; BLOCK_SIZE] {
        self.encrypt_nonce.to_le_bytes()
    }

    /// Returns the nonce used for decrypting.
    pub fn get_decrypt_nonce(&self) -> [u8; BLOCK_SIZE] {
        self.decrypt_nonce.to_le_bytes()
    }

    pub fn set_decrypt_nonce(&mut self, nonce: &[u8]) {
        self.decrypt_nonce = u128::from_le_bytes(nonce.try_into().unwrap());
        self.resync += 1;
    }

    pub fn get_crypt_setup(&self) -> CryptSetup {
        let mut crypt_setup = CryptSetup::new();

        crypt_setup.set_key(self.key.to_vec());
        crypt_setup.set_client_nonce(self.get_decrypt_nonce().to_vec());
        crypt_setup.set_server_nonce(self.get_encrypt_nonce().to_vec());

        crypt_setup
    }

    /// Encrypts an encoded voice packet and returns the resulting bytes.
    pub fn encrypt<EncodeDst: VoicePacketDst>(&mut self, packet: &VoicePacket<EncodeDst>, dst: &mut BytesMut) {
        self.encrypt_nonce = self.encrypt_nonce.wrapping_add(1);

        // Leave four bytes for header
        dst.resize(4, 0);
        let mut inner = dst.split_off(4);

        encode_voice_packet(packet, &mut inner);

        let tag = self.ocb_encrypt(inner.as_mut());
        dst.unsplit(inner);

        dst[0] = self.encrypt_nonce as u8;
        dst[1..4].copy_from_slice(&tag.to_be_bytes()[0..3]);
    }

    /// Decrypts a voice packet and (if successful) returns the `Result` of parsing the packet.
    pub fn decrypt<DecodeDst: VoicePacketDst>(&mut self, buf: &mut BytesMut) -> Result<VoicePacket<DecodeDst>, DecryptError> {
        if buf.len() < 4 {
            return Err(DecryptError::Eof);
        }
        let header = buf.split_to(4);
        let nonce_0 = header[0];

        // If we update our decrypt_nonce and the tag check fails or we've been processing late
        // packets, we need to revert it
        let saved_nonce = self.decrypt_nonce;
        let mut late = false; // will always restore nonce if this is the case
        let mut lost = 0; // for stats only

        if self.decrypt_nonce.wrapping_add(1) as u8 == nonce_0 {
            // in order
            self.decrypt_nonce = self.decrypt_nonce.wrapping_add(1);
        } else {
            // packet is late or repeated, or we lost a few packets in between
            let diff = nonce_0.wrapping_sub(self.decrypt_nonce as u8) as i8;
            self.decrypt_nonce = self.decrypt_nonce.wrapping_add(diff as u128);

            if diff > 0 {
                lost = i32::from(diff - 1); // lost a few packets in between this and the last one
            } else if diff > -30 {
                if self.decrypt_history[nonce_0 as usize] == (self.decrypt_nonce >> 8) as u8 {
                    self.decrypt_nonce = saved_nonce;

                    return Err(DecryptError::Repeat);
                }
                // just late
                late = true;
                lost = -1;
            } else {
                self.decrypt_nonce = saved_nonce;
                return Err(DecryptError::Late); // late by more than 30 packets
            }
        }

        let tag = self.ocb_decrypt(buf.as_mut());

        if Ok(()) != ring::constant_time::verify_slices_are_equal(&header[1..4], &tag.to_be_bytes()[0..3]) {
            self.decrypt_nonce = saved_nonce;
            return Err(DecryptError::Mac);
        }

        self.decrypt_history[nonce_0 as usize] = (self.decrypt_nonce >> 8) as u8;
        self.good += 1;
        self.last_good = Instant::now();

        if late {
            self.late += 1;
            self.decrypt_nonce = saved_nonce;
        }

        self.lost = (self.lost as i32 + lost) as u32;

        decode_voice_packet(buf)
    }

    /// Encrypt the provided buffer using AES-OCB, returning the tag.
    fn ocb_encrypt(&self, mut buf: &mut [u8]) -> u128 {
        let mut offset = self.aes_encrypt(self.encrypt_nonce.to_be());
        let mut checksum = 0u128;

        while buf.len() > BLOCK_SIZE {
            let (chunk, remainder) = buf.split_at_mut(BLOCK_SIZE);
            buf = remainder;
            let chunk: &mut [u8; BLOCK_SIZE] = chunk.try_into().expect("split_at works");

            offset = s2(offset);

            let plain = u128::from_be_bytes(*chunk);
            let encrypted = self.aes_encrypt(offset ^ plain) ^ offset;
            chunk.copy_from_slice(&encrypted.to_be_bytes());

            checksum ^= plain;
        }

        offset = s2(offset);

        let len = buf.len();
        assert!(len <= BLOCK_SIZE);
        let pad = self.aes_encrypt((len * 8) as u128 ^ offset);
        let mut block = pad.to_be_bytes();
        block[..len].copy_from_slice(buf);
        let plain = u128::from_be_bytes(block);
        let encrypted = pad ^ plain;
        buf.copy_from_slice(&encrypted.to_be_bytes()[..len]);

        checksum ^= plain;

        self.aes_encrypt(offset ^ s2(offset) ^ checksum)
    }

    /// Decrypt the provided buffer using AES-OCB, returning the tag.
    /// **Make sure to verify that the tag matches!**
    fn ocb_decrypt(&self, mut buf: &mut [u8]) -> u128 {
        let mut offset = self.aes_encrypt(self.decrypt_nonce.to_be());
        let mut checksum = 0u128;

        while buf.len() > BLOCK_SIZE {
            let (chunk, remainder) = buf.split_at_mut(BLOCK_SIZE);
            buf = remainder;
            let chunk: &mut [u8; BLOCK_SIZE] = chunk.try_into().expect("split_at works");

            offset = s2(offset);

            let encrypted = u128::from_be_bytes(*chunk);
            let plain = self.aes_decrypt(offset ^ encrypted) ^ offset;
            chunk.copy_from_slice(&plain.to_be_bytes());

            checksum ^= plain;
        }

        offset = s2(offset);

        let len = buf.len();
        assert!(len <= BLOCK_SIZE);
        let pad = self.aes_encrypt((len * 8) as u128 ^ offset);
        let mut block = [0; BLOCK_SIZE];
        block[..len].copy_from_slice(buf);
        let plain = u128::from_be_bytes(block) ^ pad;
        buf.copy_from_slice(&plain.to_be_bytes()[..len]);

        checksum ^= plain;

        self.aes_encrypt(offset ^ s2(offset) ^ checksum)
    }

    /// AES-128 encryption primitive.
    fn aes_encrypt(&self, data: u128) -> u128 {
        let mut data_bytes = data.to_be_bytes();
        let block = GenericArray::from_mut_slice(&mut data_bytes);
        self.aes.encrypt_block(block);

        u128::from_be_bytes(data_bytes)
    }

    /// AES-128 decryption primitive.
    fn aes_decrypt(&self, data: u128) -> u128 {
        let mut data_bytes = data.to_be_bytes();
        let block = GenericArray::from_mut_slice(&mut data_bytes);
        self.aes.decrypt_block(block);

        u128::from_be_bytes(data_bytes)
    }
}

#[inline]
fn s2(block: u128) -> u128 {
    let rot = block.rotate_left(1);
    let carry = rot & 1;
    rot ^ (carry * 0x86)
}
