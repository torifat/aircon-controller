pub mod airtouch;
pub mod api;
pub mod controller;
pub mod logic;
pub mod models;
pub mod protocol;
pub mod store;

/// Unix time in seconds, the clock everything is stamped with.
pub fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}
