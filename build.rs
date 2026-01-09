fn main() {
    prost_build::compile_protos(
        &["specs/001-signal-server/contracts/obscura.proto"],
        &["specs/001-signal-server/contracts/"],
    )
    .unwrap();
}
