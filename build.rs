fn main() {
    prost_build::compile_protos(&["proto/obscura.proto"], &["proto/"]).unwrap();
}
