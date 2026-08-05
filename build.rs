fn main() {
    prost_build::compile_protos(&["proto/ipc.proto"], &["proto/"]).unwrap();
}
