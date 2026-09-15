//! Naming settings: the template imports follow, a live preview for the editor, and
//! detecting the layout an existing library already uses.
//!
//! The editor sends the template on every keystroke and shows the result for a few
//! sample tracks chosen to exercise the tricky cases: a multi-disc album, an edition
//! name, and characters that aren't allowed in file names. Saved settings live in
//! `<data dir>/naming.json` and override the template given at startup.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use delune_core::api::ApiError;
use delune_library::layout::{self, DetectedLayout};
use delune_library::naming::{NamingOptions, TOKENS, Template, TemplateError, TrackFields};
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::accounts::CurrentUser;
use crate::review::{DEFAULT_TEMPLATE, LibrarySettings};

/// Audio files read when detecting a library's layout.
const DETECT_SAMPLE: usize = 300;

/// `GET/PUT /api/v1/naming`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NamingSettings {
    pub template: String,
    pub options: NamingOptions,
}

/// The naming settings in force.
#[derive(Debug)]
pub struct Naming {
    path: Option<PathBuf>,
    current: Mutex<(Template, NamingOptions)>,
}

impl Default for Naming {
    fn default() -> Self {
        let template = Template::parse(DEFAULT_TEMPLATE).expect("default template is valid");
        Self { path: None, current: Mutex::new((template, NamingOptions::default())) }
    }
}

impl Naming {
    /// Saved settings from `data_dir`, or the startup ones in `library`.
    #[must_use]
    pub fn open(data_dir: &Path, library: &LibrarySettings) -> Self {
        let path = data_dir.join("naming.json");
        let saved = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<NamingSettings>(&bytes).ok())
            .and_then(|s| Template::parse(&s.template).ok().map(|t| (t, s.options)));
        let current = saved.unwrap_or_else(|| (library.template.clone(), library.options.clone()));
        Self { path: Some(path), current: Mutex::new(current) }
    }

    #[must_use]
    pub fn current(&self) -> (Template, NamingOptions) {
        self.current.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    fn settings(&self) -> NamingSettings {
        let (template, options) = self.current();
        NamingSettings { template: template.as_str().to_owned(), options }
    }
}

fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(ApiError::new(code, message))).into_response()
}

/// `GET /api/v1/naming`
pub async fn get(State(app): State<AppState>, _user: CurrentUser) -> Json<NamingSettings> {
    Json(app.naming.settings())
}

/// `PUT /api/v1/naming`: save, then re-plan albums waiting in review.
pub async fn update(State(app): State<AppState>, user: CurrentUser, Json(settings): Json<NamingSettings>) -> Response {
    if let Some(denied) = user.refuse_unless(|p| p.manage, "change file naming") {
        return denied;
    }
    let template = match Template::parse(&settings.template) {
        Ok(template) => template,
        Err(e) => {
            return error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "bad-template",
                &format!("The template has a problem: {e}."),
            );
        }
    };
    let mut options = settings.options;
    options.track_padding = options.track_padding.clamp(1, 6);
    options.max_component_bytes = options.max_component_bytes.clamp(32, 255);
    if options.illegal_replacement.chars().any(|c| matches!(c, '/' | '\\' | '<' | '>' | ':' | '"' | '|' | '?' | '*')) {
        return error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "bad-replacement",
            "The replacement can't itself be a character that isn't allowed.",
        );
    }
    *app.naming.current.lock().unwrap_or_else(PoisonError::into_inner) = (template, options);
    let saved = app.naming.settings();
    if let Some(path) = &app.naming.path
        && let Err(e) =
            serde_json::to_vec_pretty(&saved).map_err(std::io::Error::other).and_then(|json| std::fs::write(path, json))
    {
        tracing::warn!(error = %e, "couldn't save naming settings");
    }
    tracing::info!(by = %user.username, template = %saved.template, "naming settings changed");
    crate::downloads::recheck_reviews(&app);
    Json(saved).into_response()
}

/// `POST /api/v1/naming/detect`: the template the library already follows.
pub async fn detect(State(app): State<AppState>, user: CurrentUser) -> Response {
    if let Some(denied) = user.refuse_unless(|p| p.manage, "change file naming") {
        return denied;
    }
    let Some(root) = app.library.library_dir.clone() else {
        return error(
            StatusCode::CONFLICT,
            "no-library",
            "Set a library folder when starting the server to detect its layout.",
        );
    };
    let detected = tokio::task::spawn_blocking(move || {
        let samples = layout::sample(&root, DETECT_SAMPLE);
        (samples.len(), layout::detect(&samples))
    })
    .await;
    match detected {
        Ok((_, Some(layout))) => Json::<DetectedLayout>(layout).into_response(),
        Ok((0, None)) => error(StatusCode::NOT_FOUND, "empty-library", "There's no music in the library folder yet."),
        Ok((_, None)) => error(
            StatusCode::NOT_FOUND,
            "no-layout",
            "Couldn't recognise a layout: the files' names don't match their tags. Pick a preset instead.",
        ),
        Err(_) => error(StatusCode::INTERNAL_SERVER_ERROR, "detect-failed", "Detection stopped unexpectedly."),
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TokenInfo {
    pub name: String,
    pub description: String,
}

/// `GET /api/v1/naming/tokens`
pub async fn tokens() -> Json<Vec<TokenInfo>> {
    Json(
        TOKENS
            .iter()
            .map(|(name, description)| TokenInfo { name: (*name).into(), description: (*description).into() })
            .collect(),
    )
}

#[derive(Debug, Deserialize)]
pub struct PreviewRequest {
    pub template: String,
    #[serde(default)]
    pub options: NamingOptions,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PreviewExample {
    /// What the sample represents, e.g. "Second disc of a double album".
    pub label: String,
    pub path: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PreviewError {
    /// 0-based character offset of the problem.
    pub position: usize,
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum PreviewResponse {
    Ok { examples: Vec<PreviewExample> },
    Invalid { error: PreviewError },
}

/// `POST /api/v1/naming/preview`
pub async fn preview(Json(request): Json<PreviewRequest>) -> (StatusCode, Json<PreviewResponse>) {
    match Template::parse(&request.template) {
        Ok(template) => {
            let examples = samples()
                .into_iter()
                .map(|(label, fields)| PreviewExample {
                    label: label.into(),
                    path: format!("{}.{}", template.render(&fields, &request.options), fields.codec.to_lowercase()),
                })
                .collect();
            (StatusCode::OK, Json(PreviewResponse::Ok { examples }))
        }
        Err(TemplateError { position, message }) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(PreviewResponse::Invalid { error: PreviewError { position, message } }),
        ),
    }
}

fn samples() -> Vec<(&'static str, TrackFields)> {
    let base = TrackFields {
        title: "Paranoid Android".into(),
        artist: "Radiohead".into(),
        album_artist: "Radiohead".into(),
        album: "OK Computer".into(),
        year: Some(1997),
        track: 2,
        disc: 1,
        disc_count: 1,
        genre: Some("Alternative Rock".into()),
        label: Some("Parlophone".into()),
        catalog: Some("NODATA 02".into()),
        isrc: Some("GBAYE9700378".into()),
        codec: "FLAC".into(),
        quality: "FLAC 16/44.1".into(),
        bit_depth: Some(16),
        sample_rate: Some(44_100),
        bitrate: Some(1_031),
        source: "Soulseek".into(),
        mbid: Some("0b6b4ba0-d36f-47bd-b4ea-6a5b91842d29".into()),
        ..TrackFields::default()
    };
    vec![
        ("A track on a single-disc album", base.clone()),
        (
            "Second disc of a deluxe edition",
            TrackFields {
                title: "Lift".into(),
                album: "OK Computer OKNOTOK 1997 2017".into(),
                edition: Some("Remastered".into()),
                year: Some(2017),
                track: 3,
                disc: 2,
                disc_count: 2,
                tracks_before_disc: 12,
                quality: "FLAC 24/96".into(),
                bit_depth: Some(24),
                sample_rate: Some(96_000),
                ..base.clone()
            },
        ),
        (
            "Characters that aren't allowed in file names",
            TrackFields {
                title: "Whole Lotta Rosie / Live?".into(),
                artist: "AC/DC".into(),
                album_artist: "AC/DC".into(),
                album: "If You Want Blood...".into(),
                year: Some(1978),
                track: 7,
                codec: "MP3".into(),
                quality: "MP3 320".into(),
                bit_depth: None,
                sample_rate: None,
                bitrate: Some(320),
                ..base
            },
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn run(template: &str) -> (StatusCode, PreviewResponse) {
        let (status, Json(body)) =
            preview(Json(PreviewRequest { template: template.into(), options: NamingOptions::default() })).await;
        (status, body)
    }

    #[tokio::test]
    async fn previews_every_sample() {
        let (status, body) = run("{album_artist}/{year} - {album}[ ({edition})]/{track} - {title}").await;
        assert_eq!(status, StatusCode::OK);
        let PreviewResponse::Ok { examples } = body else { panic!("expected examples") };
        let paths: Vec<_> = examples.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "Radiohead/1997 - OK Computer/02 - Paranoid Android.flac",
                "Radiohead/2017 - OK Computer OKNOTOK 1997 2017 (Remastered)/2-03 - Lift.flac",
                "AC_DC/1978 - If You Want Blood/07 - Whole Lotta Rosie _ Live_.mp3",
            ]
        );
    }

    #[tokio::test]
    async fn reports_where_the_template_is_wrong() {
        let (status, body) = run("{album} - {titel}").await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            body,
            PreviewResponse::Invalid {
                error: PreviewError { position: 10, message: "unknown token `{titel}`".into() }
            }
        );
    }
}
