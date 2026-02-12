pub mod obscura {
    #[allow(clippy::all)]
    #[allow(clippy::pedantic)]
    #[allow(clippy::nursery)]
    pub mod v1 {
        include!(concat!(env!("OUT_DIR"), "/obscura.v1.rs"));
    }
}
