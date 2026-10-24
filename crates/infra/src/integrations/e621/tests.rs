use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use url::Url;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use lukosbot_integrations::E621Rating;

use super::dto::*;
use super::*;

#[test]
fn parses_artist_urls() {
    let dto: ArtistDto = serde_json::from_str(
        r#"{
            "id": 1,
            "name": "A",
            "other_names": ["B"],
            "notes": "notes",
            "group_name": "group",
            "urls": [
                "https://example.com/a",
                {"url": "https://example.com/b", "normalized_url": "https://example.com/b"}
            ],
            "is_active": true
        }"#,
    )
    .unwrap();
    let artist = dto.into_model();
    assert_eq!(artist.other_names, vec!["B"]);
    assert_eq!(artist.urls.len(), 2);
    assert_eq!(artist.group_name.as_deref(), Some("group"));
}

#[test]
fn parses_v2_post_thumbnail_priority() {
    let dto: PostDto = serde_json::from_str(
        r#"{
            "id": 2,
            "rating": "s",
            "files": {
                "original": {"width": 10, "height": 20, "url": "https://static1.e621.net/a.png"},
                "preview": {"jpg": "https://static1.e621.net/p.jpg", "webp": null},
                "sample": {"jpg": "https://static1.e621.net/s.jpg", "webp": null},
                "meta": {"md5": "abc", "ext": "png", "size": 42}
            },
            "stats": {"score": {"up": 2, "down": 1, "total": 3}, "fav_count": 4, "comment_count": 5},
            "tags": {"artist": ["a"]}
        }"#,
    )
    .unwrap();
    let post = dto.into_model();
    assert_eq!(post.rating, E621Rating::Safe);
    assert_eq!(
        post.thumbnail_url().map(Url::as_str),
        Some("https://static1.e621.net/p.jpg")
    );
    assert_eq!(post.score, Some(3));
    assert_eq!(post.favorites, Some(4));
}

#[test]
fn parses_legacy_post_as_fallback() {
    let response: SinglePostResponse = serde_json::from_str(
        r#"{"post": {"id": 3, "rating": "q", "file": {"url": "https://static1.e621.net/a.png"}}}"#,
    )
    .unwrap();
    let post = response.into_post().into_model();
    assert_eq!(post.id, 3);
    assert_eq!(post.rating, E621Rating::Questionable);
}

#[tokio::test]
async fn artist_detail_uses_the_artists_path() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/artists/128713.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": 128713,
            "name": "maudoggo",
            "other_names": [],
            "urls": [],
            "is_active": true
        })))
        .mount(&server)
        .await;

    let http = Arc::new(
        HttpClient::new(crate::http::HttpSettings {
            request_timeout: Duration::from_secs(2),
            ..crate::http::HttpSettings::default()
        })
        .unwrap(),
    );
    let client =
        E621Client::with_base_url(http, Url::parse(&format!("{}/", server.uri())).unwrap());
    let artist = client.get_artist("128713").await.unwrap().unwrap();
    assert_eq!(artist.id, 128713);
    assert_eq!(artist.name, "maudoggo");
}

#[tokio::test]
async fn v2_search_is_anonymous_and_extended() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/posts.json"))
        .and(query_param("v2", "true"))
        .and(query_param("mode", "extended"))
        .and(query_param("tags", "cat"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{
            "id": 7,
            "rating": "s",
            "files": {
                "original": {"width": 1, "height": 2, "url": "https://static1.e621.net/a.png"},
                "preview": {"jpg": "https://static1.e621.net/p.jpg", "webp": null},
                "sample": {"jpg": null, "webp": null},
                "meta": {"md5": "abc", "ext": "png", "size": 9}
            },
            "stats": {"score": {"up": 1, "down": 0, "total": 1}, "fav_count": 2, "comment_count": 3},
            "tags": {"artist": ["a"]},
            "sources": ["https://example.com"]
        }])))
        .mount(&server)
        .await;

    let http = Arc::new(
        HttpClient::new(crate::http::HttpSettings {
            request_timeout: Duration::from_secs(2),
            ..crate::http::HttpSettings::default()
        })
        .unwrap(),
    );
    let client =
        E621Client::with_base_url(http, Url::parse(&format!("{}/", server.uri())).unwrap());
    let posts = client
        .search_posts(
            "cat",
            NonZeroU32::new(1).unwrap(),
            NonZeroU32::new(12).unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0].sources, vec!["https://example.com"]);
    let requests = server.received_requests().await.unwrap();
    assert!(requests[0].headers.get("authorization").is_none());
}

#[test]
fn artist_endpoint_is_the_expected_url() {
    let client = E621Client::with_base_url(
        Arc::new(HttpClient::new(crate::http::HttpSettings::default()).unwrap()),
        Url::parse("https://e621.net/").unwrap(),
    );
    assert_eq!(
        client
            .endpoint(&["artists", "128713.json"])
            .unwrap()
            .as_str(),
        "https://e621.net/artists/128713.json"
    );
}
