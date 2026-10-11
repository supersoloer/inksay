pub fn metadata() -> serde_json::Value {
    serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "build": env!("INKSAY_BUILD_NUMBER"),
        "commit": env!("INKSAY_GIT_COMMIT"),
        "dirty": env!("INKSAY_GIT_DIRTY") == "true",
        "rustVersion": env!("INKSAY_RUST_VERSION"),
        "target": env!("INKSAY_TARGET"),
        "dataSchemaVersion": crate::persistence::SCHEMA_VERSION,
    })
}
