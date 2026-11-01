pub mod airtouch;
pub mod api;
pub mod controller;
pub mod logic;
pub mod models;
pub mod protocol;
pub mod store;

/// The release this was built from, e.g. "0.3.0", passed in by the image build; "dev" for
/// local builds.
pub fn version() -> &'static str {
    option_env!("AIRCON_VERSION")
        .filter(|v| !v.is_empty())
        .unwrap_or("dev")
}

/// The commit this was built from, shortened, if the build passed one in.
pub fn commit() -> Option<&'static str> {
    option_env!("AIRCON_COMMIT")
        .filter(|c| !c.is_empty())
        .map(|c| &c[..c.len().min(7)])
}

/// Unix time in seconds, the clock everything is stamped with.
pub fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}
