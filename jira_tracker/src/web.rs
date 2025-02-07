use std::fmt::Debug;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::app_data::{AppData, TrackerError};
use crate::config::LogError;
use crate::jira_api::JiraApi;
use crate::tempo_api::TempoApi;
use crate::AppState;
use domain::TrackerInformation;

async fn list(State(state): State<Arc<AppData>>) -> Json<Vec<TrackerInformation>> {
    Json(state.list_trackers())
}

async fn get_tracker(
    Path(key): Path<String>,
    State(state): State<Arc<AppData>>,
) -> Result<Json<TrackerInformation>, TrackerError> {
    state.get_tracker(&key).map(Json)
}

async fn create(
    Path(key): Path<String>,
    State(jira): State<Arc<JiraApi>>,
    State(state): State<Arc<AppData>>,
) -> Result<Json<TrackerInformation>, TrackerError> {
    let issue = jira
        .get_issue_info(&key)
        .await
        .map_err(|_| TrackerError::NotFoundError)?;
    state.create_tracker(&key, &issue.id)?;
    let tracker = state.start(&key)?;
    Ok(Json(tracker))
}

async fn start(
    Path(key): Path<String>,
    State(state): State<Arc<AppData>>,
) -> Result<Json<TrackerInformation>, TrackerError> {
    state.start(&key).map(Json)
}

#[derive(Debug, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
enum AdjustTrackerBody {
    SetDescription {
        description: Option<String>,
    },
    PositiveDuration {
        #[serde(
            rename = "plus",
            alias = "add",
            alias = "increase",
            with = "humantime_serde"
        )]
        duration: Duration,
        #[serde(alias = "from")]
        using: Option<String>,
    },
    NegativeDuration {
        #[serde(
            rename = "minus",
            alias = "sub",
            alias = "subtract",
            alias = "decrease",
            with = "humantime_serde"
        )]
        duration: Duration,
        #[serde(alias = "to")]
        using: Option<String>,
    },
}

async fn adjust(
    Path(key): Path<String>,
    State(state): State<Arc<AppData>>,
    Json(body): Json<AdjustTrackerBody>,
) -> Result<Json<TrackerInformation>, TrackerError> {
    let tracker = match body {
        AdjustTrackerBody::SetDescription { description } => {
            state.set_description(&key, description)?
        }
        AdjustTrackerBody::PositiveDuration { duration, using } => {
            if let Some(other_key) = using {
                state.adjust_negative_duration(&other_key, duration)?;
            }
            state.adjust_positive_duration(&key, duration)?
        }
        AdjustTrackerBody::NegativeDuration { duration, using } => {
            let tracker = state.adjust_negative_duration(&key, duration)?;
            if let Some(other_key) = using {
                state.adjust_positive_duration(&other_key, duration)?;
            }
            tracker
        }
    };
    Ok(Json(tracker))
}

async fn delete(
    Path(key): Path<String>,
    State(state): State<Arc<AppData>>,
) -> Result<StatusCode, TrackerError> {
    state.remove(&key).map(|_| StatusCode::NO_CONTENT)
}

async fn clear(State(state): State<Arc<AppData>>) -> StatusCode {
    state.remove_all();
    StatusCode::NO_CONTENT
}

async fn current(
    State(state): State<Arc<AppData>>,
) -> Result<Json<TrackerInformation>, TrackerError> {
    state.current().map(Json)
}

async fn pause(State(state): State<Arc<AppData>>) {
    state.pause()
}

#[derive(Debug, Serialize)]
struct SumResponse {
    #[serde(with = "humantime_serde")]
    duration: Duration,
}

async fn sum(State(state): State<Arc<AppData>>) -> Json<SumResponse> {
    Json(SumResponse {
        duration: state.sum(),
    })
}

async fn submit(
    State(state): State<Arc<AppData>>,
    State(api): State<Arc<TempoApi>>,
) -> Result<(), LogError> {
    api.submit_all(state.list_trackers()).await?;
    state.remove_all();
    Ok(())
}

pub fn router() -> Router<AppState> {
    let trackers_routes = Router::new()
        .route("/", get(list).delete(clear))
        .route(
            "/:key",
            get(get_tracker).post(create).put(adjust).delete(delete),
        )
        .route("/:key/start", post(start));

    let tracker_routes = Router::new()
        .route("/", get(current))
        .route("/pause", post(pause));

    Router::new()
        .nest("/trackers", trackers_routes)
        .nest("/tracker", tracker_routes)
        .route("/sum", get(sum))
        .route("/submit", post(submit))
}
