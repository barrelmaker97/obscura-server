#![allow(clippy::unwrap_used, clippy::panic, clippy::todo, clippy::missing_panics_doc, clippy::must_use_candidate, missing_debug_implementations, clippy::cast_precision_loss, clippy::clone_on_ref_ptr, clippy::match_same_arms, clippy::items_after_statements, unreachable_pub, clippy::print_stdout, clippy::similar_names)]
fn main() {
    prost_build::compile_protos(&["proto/obscura/v1/obscura.proto"], &["proto/"]).unwrap();
}
