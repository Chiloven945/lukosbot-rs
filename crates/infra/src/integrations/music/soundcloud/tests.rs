use std::sync::Arc;
use std::time::Duration;

use secrecy::SecretString;
use serde_json::json;
use url::Url;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use lukosbot_integrations::{IntegrationError, parse_music_link};

use super::*;
use crate::http::{HttpClient, HttpSettings};

fn http() -> Arc<HttpClient> {
    Arc::new(
        HttpClient::new(HttpSettings {
            connect_timeout: Duration::from_secs(2),
            request_timeout: Duration::from_secs(2),
            max_response_bytes: 1024 * 1024,
            user_agent: "LukosBot-tests/0.1".to_owned(),
            proxy: None,
        })
        .unwrap(),
    )
}

fn client(server: &MockServer) -> SoundCloudClient {
    SoundCloudClient::with_api_base(
        http(),
        SecretString::from("client-id".to_owned()),
        Url::parse(&format!("{}/", server.uri())).unwrap(),
    )
}

fn track() -> serde_json::Value {
    json!({
        "id": 42,
        "title": "Track",
        "publisher_metadata": {"artist": "Artist", "album_title": "Album"},
        "user": {"username": "Fallback"},
        "artwork_url": "https://example.com/art.jpg",
        "permalink_url": "https://soundcloud.com/artist/track",
        "full_duration": 213000
    })
}

#[tokio::test]
async fn searches_and_maps_soundcloud_tracks() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search/tracks"))
        .and(query_param("q", "track"))
        .and(query_param("client_id", "client-id"))
        .and(query_param("limit", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "collection": [track()]
        })))
        .mount(&server)
        .await;

    let result = client(&server).search("track").await.unwrap();
    assert_eq!(result.title, "Track");
    assert_eq!(result.artist, "Artist");
    assert_eq!(result.album.as_deref(), Some("Album"));
}

#[tokio::test]
async fn resolves_links_and_uses_artist_fallback() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/resolve"))
        .and(query_param("client_id", "client-id"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": 43,
            "title": "Resolved",
            "user": {"username": "Fallback"},
            "full_duration": 0
        })))
        .mount(&server)
        .await;

    let link = parse_music_link("https://soundcloud.com/artist/track").unwrap();
    let result = client(&server).resolve_link(&link).await.unwrap();
    assert_eq!(result.artist, "Fallback");
    assert_eq!(result.duration, None);
}

#[tokio::test]
async fn invalid_client_is_unauthorized() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search/tracks"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;

    let error = client(&server).search("track").await.unwrap_err();
    assert!(matches!(error, IntegrationError::Unauthorized));
}
