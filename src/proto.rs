pub mod obscura {
    pub mod v1 {
        include!(concat!(env!("OUT_DIR"), "/obscura.v1.rs"));
    }
}
