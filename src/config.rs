use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use std::error::Error;
use std::ffi::OsStr;
use std::fmt::Display;
use std::fs::File;
use std::io::{BufReader, BufWriter, Write};
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::{env, fs, io};
use tracing::info_span;

use hotwatch::notify::DebouncedEvent;
use hotwatch::Hotwatch;
use serde::de::DeserializeOwned;
use static_init::dynamic;

use crate::app_data::AppData;

fn env_var<K: Display + AsRef<OsStr>>(key: K) -> String {
    env::var(&key).unwrap_or_else(|_| panic!("env var `{}` not set", key))
}

#[dynamic]
pub static JIRA_ACCOUNT_ID: String = env_var("JIRA_ACCOUNT_ID");
#[dynamic]
pub static TEMPO_API_TOKEN: String = env_var("TEMPO_API_TOKEN");
#[dynamic]
static JSON_FILE: String = shellexpand::full(&env_var("JSON_FILE"))
    .unwrap()
    .into_owned();

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

pub fn get_initial_state() -> AppData {
    read_state_file().unwrap_or_else(|_| AppData::new())
}

pub fn read_state_file<F>() -> Result<F, io::Error>
where
    F: DeserializeOwned,
{
    let file = File::open(get_state_file())?;
    let reader = BufReader::new(file);
    let app_data = serde_json::from_reader(reader)?;
    Ok(app_data)
}

pub fn write_state_file(app_data: &AppData) -> Result<(), io::Error> {
    let file_path = get_state_file();
    let parent_directory = file_path.parent().unwrap();
    fs::create_dir_all(parent_directory)?;
    let file = File::create(file_path)?;

    let mut writer = BufWriter::new(file);
    serde_json::to_writer_pretty(&mut writer, app_data)?;
    writer.flush()?;
    Ok(())
}

#[must_use]
pub fn watch_state_file<F>(f: F) -> Hotwatch
where
    F: 'static + Fn() + Send,
{
    let watched_path = get_state_file();

    let span = info_span!(
        "watch_state_file",
        path = watched_path
            .canonicalize()
            .as_ref()
            .unwrap_or(&watched_path)
            .to_string_lossy()
            .into_owned()
    );
    let _enter = span.clone().entered();

    let parent = watched_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .canonicalize()
        .unwrap_or_else(|_| panic!("Parent path for {} does not exist.", watched_path.display()));
    let mut hotwatch = Hotwatch::new_with_custom_delay(Duration::from_secs(1)).unwrap();
    hotwatch
        .watch(parent, move |event| match event {
            DebouncedEvent::Create(event_path)
            | DebouncedEvent::Write(event_path)
            | DebouncedEvent::Chmod(event_path)
            | DebouncedEvent::Rename(_, event_path) => {
                if let Ok(path) = event_path.canonicalize() {
                    if Some(path) == watched_path.canonicalize().ok() {
                        span.in_scope(|| {
                            tracing::debug!("loading state from file");
                        });
                        f();
                    }
                }
            }
            _ => {}
        })
        .unwrap();

    tracing::debug!("started monitoring of state file");

    hotwatch
}

fn get_state_file() -> PathBuf {
    PathBuf::from(JSON_FILE.deref())
}
