//! Naming template preview, for the settings editor.
//!
//! The editor sends the template on every keystroke and shows the result for a few
//! sample tracks chosen to exercise the tricky cases: a multi-disc album, an edition
//! name, and characters that aren't allowed in file names.

use axum::{Json, http::StatusCode};
use delune_library::naming::{NamingOptions, TOKENS, Template, TemplateError, TrackFields};
use serde::{Deserialize, Serialize};

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
