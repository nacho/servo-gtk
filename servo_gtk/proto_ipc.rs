/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use prost::Message;

include!(concat!(env!("OUT_DIR"), "/servo_ipc.rs"));

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
