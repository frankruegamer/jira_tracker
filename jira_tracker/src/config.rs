use std::error::Error;
use std::path::PathBuf;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use figment::providers::Env;
use figment::Figment;
use serde::{Deserialize, Deserializer};
use tower_http::classify::{ServerErrorsAsFailures, SharedClassifier};
use tower_http::trace::TraceLayer;
use tracing::Level;
use tracing_subscriber::filter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

const DEFAULT_PORT: fn() -> u16 = || 8080;

fn deserialize_path<'de, D>(deserializer: D) -> Result<PathBuf, D::Error>
where
    D: Deserializer<'de>,
{
    let string = String::deserialize(deserializer)?;
    Ok(PathBuf::from(shellexpand::full(&string).unwrap().as_ref()))
}

#[derive(Debug, Deserialize)]
pub struct AppConfig {
    pub jira_email: String,
    pub jira_api_token: String,
    pub tempo_api_token: String,
    #[serde(default = "DEFAULT_PORT")]
    pub tracker_port: u16,
    #[serde(deserialize_with = "deserialize_path")]
    pub json_file: PathBuf,
}

impl AppConfig {
    pub fn new() -> Self {
        let figment = Figment::from(Env::raw());
        figment.extract().unwrap()
    }
}

pub struct LogError(Box<dyn Error>);

impl<E> From<E> for LogError
where
    E: Error + 'static,
{
    fn from(value: E) -> Self {
        LogError(Box::new(value))
    }
}

impl IntoResponse for LogError {
    fn into_response(self) -> Response {
        let LogError(error) = self;
        eprintln!("Internal Server Error: {}", error);
        StatusCode::INTERNAL_SERVER_ERROR.into_response()
    }
}

#[must_use]
pub fn setup_logging() -> TraceLayer<SharedClassifier<ServerErrorsAsFailures>> {
    let targets = filter::Targets::new()
        .with_target("tower_http::trace::on_request", Level::DEBUG)
        .with_target("tower_http::trace::make_span", Level::DEBUG)
        .with_target("jira_tracker", Level::DEBUG)
        .with_default(Level::INFO);

    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(targets)
        .init();

    TraceLayer::new_for_http()
}
