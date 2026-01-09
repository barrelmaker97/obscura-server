use std::sync::Once;

static INIT: Once = Once::new();

pub fn setup_tracing() {
    INIT.call_once(|| {
        tracing_subscriber::fmt()
            .with_env_filter("debug")
            .init();
    });
}