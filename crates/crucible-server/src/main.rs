//! CRUCIBLE server.
//!
//! Serves the static client, REST endpoints for dashboard data/replays, and a
//! WebSocket live-match endpoint. The trainer and dashboard land in M4–M6.

mod http;
mod store;
mod trainer;
mod ws;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde_json::json;
use tower_http::services::ServeDir;

use store::Store;
use trainer::{TrainerConfig, TrainerShared};

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) store: Arc<Store>,
    pub(crate) trainer: Arc<TrainerShared>,
    pub(crate) started_at: std::time::Instant,
}

/// Start the 24/7 trainer if `CRUCIBLE_TRAINER=1`. Optional
/// `CRUCIBLE_TRAINER_GENERATIONS=N` runs a bounded fast-forward; `SMALL=1` uses
/// a small, fast configuration.
fn start_trainer(store: Arc<Store>, shared: Arc<TrainerShared>) {
    let enabled = std::env::var("CRUCIBLE_TRAINER")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("on"))
        .unwrap_or(false);
    if !enabled {
        return;
    }
    let generations: Option<usize> = std::env::var("CRUCIBLE_TRAINER_GENERATIONS")
        .ok()
        .and_then(|s| s.parse().ok());
    let small = std::env::var("CRUCIBLE_TRAINER_SMALL")
        .map(|v| v == "1")
        .unwrap_or(false);
    let mut cfg = if small {
        TrainerConfig::small()
    } else {
        TrainerConfig::default()
    };
    // `CRUCIBLE_TRAINER_BOOTSTRAP=1` runs the staged curriculum (plan §5.7)
    // on a cold start, so the self-play loop begins from a competent
    // population and a champion that already beats the hard bot.
    if std::env::var("CRUCIBLE_TRAINER_BOOTSTRAP")
        .map(|v| v == "1")
        .unwrap_or(false)
    {
        cfg.bootstrap = true;
    }

    tokio::task::spawn_blocking(move || {
        shared
            .running
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let mut trainer = match trainer::Trainer::start(store, shared.clone(), cfg) {
            Ok(t) => t,
            Err(e) => {
                tracing::error!("trainer failed to start: {e}");
                shared
                    .running
                    .store(false, std::sync::atomic::Ordering::Relaxed);
                return;
            }
        };
        let mut n = 0usize;
        loop {
            match trainer.run_generation() {
                Ok(Some(p)) => tracing::info!(
                    "promoted genome {} (gen {}, Elo {:.0}, {:.0}% vs champion)",
                    p.genome_id,
                    p.generation,
                    p.elo,
                    p.gauntlet.champion_win_rate * 100.0
                ),
                Ok(None) => {}
                Err(e) => {
                    tracing::error!("trainer error: {e}");
                    break;
                }
            }
            n += 1;
            if let Some(limit) = generations {
                if n >= limit {
                    break;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        shared
            .running
            .store(false, std::sync::atomic::Ordering::Relaxed);
        tracing::info!("trainer finished after {n} generations");
    });
}

async fn hello() -> impl IntoResponse {
    Json(json!({ "service": "crucible-server", "sim": crucible_sim::VERSION }))
}

async fn health(State(state): State<AppState>) -> impl IntoResponse {
    Json(json!({
        "ok": true,
        "uptime_secs": state.started_at.elapsed().as_secs(),
    }))
}

async fn list_replays(State(state): State<AppState>) -> impl IntoResponse {
    match state.store.list_matches(100) {
        Ok(list) => Json(json!({ "matches": list })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn get_replay(State(state): State<AppState>, Path(id): Path<i64>) -> impl IntoResponse {
    match state.store.get_replay(id) {
        Ok(Some(replay)) => Json(json!({ "replay": replay })).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "no such replay").into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_env_filter("info").init();

    let db_path = std::env::var("CRUCIBLE_DB").unwrap_or_else(|_| "data/crucible.db".into());
    if let Some(parent) = std::path::Path::new(&db_path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let store = Arc::new(Store::open(&db_path).expect("failed to open SQLite store"));
    let trainer_shared = Arc::new(TrainerShared::default());
    // Surface the checkpointed generation in /api/status before the trainer
    // (if enabled) resumes.
    if let Ok(Some(gen)) = store.latest_generation() {
        trainer_shared
            .generation
            .store(gen, std::sync::atomic::Ordering::Relaxed);
    }
    start_trainer(store.clone(), trainer_shared.clone());

    let state = AppState {
        store,
        trainer: trainer_shared,
        started_at: std::time::Instant::now(),
    };

    let static_dir = std::env::var("CRUCIBLE_CLIENT_DIR").unwrap_or_else(|_| "client/dist".into());
    tracing::info!("serving static client from {static_dir}");

    let app = Router::new()
        .route("/api/hello", get(hello))
        .route("/api/health", get(health))
        .route("/api/replays", get(list_replays))
        .route("/api/replay/{id}", get(get_replay))
        .route("/api/champion", get(http::champion))
        .route("/api/elo-history", get(http::elo_history))
        .route("/api/lineage/{id}", get(http::lineage))
        .route("/api/museum", get(http::museum))
        .route("/api/status", get(http::status))
        .route("/api/training-stats", get(http::training_stats))
        .route("/api/report/{old}/{new}", get(http::report))
        .route("/api/autobattle/{a}/{b}", get(http::autobattle))
        .route("/ws", get(ws::handler))
        .fallback_service(ServeDir::new(&static_dir).append_index_html_on_directories(true))
        .with_state(state);

    let addr: SocketAddr = std::env::var("CRUCIBLE_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:8787".into())
        .parse()
        .expect("invalid CRUCIBLE_ADDR");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind");
    tracing::info!("listening on http://{addr}");
    axum::serve(listener, app).await.expect("server error");
}
