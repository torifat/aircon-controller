//! `aircon`: the controller service.
//!
//! Configuration is by environment variable:
//!   AIRCON_AIRTOUCH_HOST  AirTouch 5 console IP (required)
//!   AIRCON_DB_PATH        SQLite file (default ./data/aircon.db)
//!   AIRCON_UI_DIR         built UI to serve at / (optional)
//!   AIRCON_LISTEN         listen address (default 0.0.0.0:8080)
//!   RUST_LOG              log filter (default info)
//! Everything tunable at runtime (dry run, deadband, ...) lives in the database and is
//! changed through the API.

use std::path::PathBuf;
use std::sync::Arc;

use aircon_server::airtouch::AirTouch;
use aircon_server::controller::Controller;
use aircon_server::store::Store;
use aircon_server::{api, now};
use anyhow::Context;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let host = std::env::var("AIRCON_AIRTOUCH_HOST")
        .context("AIRCON_AIRTOUCH_HOST is required (the AirTouch 5 console's IP)")?;
    let db =
        PathBuf::from(std::env::var("AIRCON_DB_PATH").unwrap_or_else(|_| "data/aircon.db".into()));
    let ui_dir = std::env::var_os("AIRCON_UI_DIR").map(PathBuf::from);
    let listen = std::env::var("AIRCON_LISTEN").unwrap_or_else(|_| "0.0.0.0:8080".into());

    let airtouch = AirTouch::new(host);
    let controller = Controller::new(Arc::clone(&airtouch), Store::open(&db)?, Arc::new(now));
    tokio::spawn(airtouch.run());
    tokio::spawn(Arc::clone(&controller).run());

    let listener = tokio::net::TcpListener::bind(&listen)
        .await
        .with_context(|| format!("binding {listen}"))?;
    tracing::info!("listening on {listen}");
    axum::serve(listener, api::router(controller, ui_dir))
        .with_graceful_shutdown(shutdown())
        .await?;
    Ok(())
}

async fn shutdown() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {}
        () = term => {}
    }
}
