use prost::Message;

include!(concat!(env!("OUT_DIR"), "/servo_ipc.rs"));

impl ServoAction {
    pub fn encode_to_vec(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        self.encode(&mut buf).unwrap();
        buf
    }

    pub fn decode_from_slice(buf: &[u8]) -> Result<Self, prost::DecodeError> {
        Self::decode(buf)
    }
}

impl ServoEvent {
    pub fn encode_to_vec(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        self.encode(&mut buf).unwrap();
        buf
    }

    pub fn decode_from_slice(buf: &[u8]) -> Result<Self, prost::DecodeError> {
        Self::decode(buf)
    }
}
