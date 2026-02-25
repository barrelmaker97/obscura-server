fn main() {
    prost_build::compile_protos(&["proto/obscura/v1/obscura.proto"], &["proto/"]).expect("Failed to compile protos");
}
