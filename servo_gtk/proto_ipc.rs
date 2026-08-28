/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use prost::Message;

include!(concat!(env!("OUT_DIR"), "/servo_ipc.rs"));

/// Encode `message` into a single buffer prefixed with its little-endian `u32`
/// length, ready to be handed to one `write_all`.
///
/// Reserving the prefix up front keeps this to a single allocation sized by
/// `encoded_len`, which matters for frame messages that carry a whole
/// framebuffer: growing a `Vec` from empty reallocates and copies the payload
/// repeatedly, and writing the prefix separately doubles the syscalls.
pub fn encode_framed<M: Message>(message: &M) -> Vec<u8> {
    let mut buf = Vec::with_capacity(4 + message.encoded_len());
    buf.extend_from_slice(&[0; 4]);
    message
        .encode(&mut buf)
        .expect("encoding into a Vec cannot fail");
    let len = (buf.len() - 4) as u32;
    buf[..4].copy_from_slice(&len.to_le_bytes());
    buf
}

impl ServoAction {
    pub fn decode_from_slice(buf: &[u8]) -> Result<Self, prost::DecodeError> {
        Self::decode(buf)
    }
}

impl ServoEvent {
    pub fn decode_from_slice(buf: &[u8]) -> Result<Self, prost::DecodeError> {
        Self::decode(buf)
    }
}
