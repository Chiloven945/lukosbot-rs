use std::sync::Arc;
use std::time::Duration;

use secrecy::SecretString;
use serde_json::json;
use url::Url;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use lukosbot_integrations::{MusicPlatform, parse_music_link};

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

fn client(server: &MockServer) -> SpotifyClient {
    SpotifyClient::with_endpoints(
        http(),
        SecretString::from("client".to_owned()),
        SecretString::from("secret".to_owned()),
        Url::parse(&format!("{}/token", server.uri())).unwrap(),
        Url::parse(&format!("{}/v1", server.uri())).unwrap(),
    )
}

fn track() -> serde_json::Value {
    json!({
        "id": "trackid",
        "name": "Track",
        "artists": [{"name": "Artist"}],
        "album": {
            "name": "Album",
            "images": [{"url": "https://example.com/cover.jpg"}]
        },
        "external_urls": {"spotify": "https://open.spotify.com/track/trackid"},
        "duration_ms": 213000
    })
}

#[tokio::test]
async fn caches_the_client_credentials_token() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "access", "expires_in": 3600
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/search"))
        .and(query_param("q", "track"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "tracks": {"items": [track()]}
        })))
        .expect(2)
        .mount(&server)
        .await;

    let client = client(&server);
    let first = client.search("track").await.unwrap();
    let second = client.search("track").await.unwrap();
    assert_eq!(first.title, "Track");
    assert_eq!(second.artist, "Artist");
    assert_eq!(second.duration, Some(Duration::from_millis(213_000)));
}

#[tokio::test]
async fn concurrent_searches_refresh_the_token_only_once() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "access", "expires_in": 3600
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "tracks": {"items": [track()]}
        })))
        .expect(2)
        .mount(&server)
        .await;

    let client = Arc::new(client(&server));
    let (first, second) = tokio::join!(client.search("track"), client.search("track"));
    assert_eq!(first.unwrap().title, "Track");
    assert_eq!(second.unwrap().title, "Track");
}

#[tokio::test]
async fn resolves_a_track_link() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "access", "expires_in": 3600
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/tracks/trackid"))
        .respond_with(ResponseTemplate::new(200).set_body_json(track()))
        .mount(&server)
        .await;

    let link = parse_music_link("https://open.spotify.com/track/trackid").unwrap();
    let result = client(&server).resolve_link(&link).await.unwrap();
    assert_eq!(result.title, "Track");
}

#[tokio::test]
async fn refreshes_once_after_unauthorized() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "access", "expires_in": 3600
        })))
        .expect(2)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/search"))
        .respond_with(ResponseTemplate::new(401))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "tracks": {"items": [track()]}
        })))
        .mount(&server)
        .await;

    let result = client(&server).search("track").await.unwrap();
    assert_eq!(result.platform, MusicPlatform::Spotify);
}

#[tokio::test]
async fn empty_search_is_not_found() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "access", "expires_in": 3600
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "tracks": {"items": []}
        })))
        .mount(&server)
        .await;

    let error = client(&server).search("missing").await.unwrap_err();
    assert!(matches!(error, IntegrationError::NotFound));
}
