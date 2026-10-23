use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use url::Url;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use lukosbot_integrations::{BilibiliService, BilibiliTarget, IntegrationError, VideoId};

use super::*;
use crate::http::{HttpClient, HttpSettings, RedirectMode};

fn http(redirect: RedirectMode) -> Arc<HttpClient> {
    Arc::new(
        HttpClient::new_with_redirect(
            HttpSettings {
                connect_timeout: Duration::from_secs(2),
                request_timeout: Duration::from_secs(2),
                max_response_bytes: 1024 * 1024,
                user_agent: "LukosBot-tests/0.1".to_owned(),
                proxy: None,
            },
            redirect,
        )
        .unwrap(),
    )
}

fn endpoints(server: &MockServer) -> (Url, Url) {
    (
        Url::parse(&format!("{}/view", server.uri())).unwrap(),
        Url::parse(&format!("{}/relation", server.uri())).unwrap(),
    )
}

fn client(server: &MockServer, allowed: Vec<String>) -> BilibiliClient {
    let (view, relation) = endpoints(server);
    BilibiliClient::with_endpoints(
        http(RedirectMode::Follow),
        http(RedirectMode::None),
        view,
        relation,
        allowed,
    )
}

fn view_body() -> serde_json::Value {
    json!({
        "code": 0,
        "data": {
            "bvid": "BV1GJ411x7h7",
            "title": "Example",
            "tname": "科技",
            "desc": "Description",
            "pic": "https://example.com/cover.jpg",
            "pubdate": 1_700_000_000,
            "owner": {"mid": 42, "name": "Owner"},
            "stat": {
                "view": 1000, "danmaku": 20, "reply": 30,
                "favorite": 40, "coin": 50, "share": 60, "like": 70
            },
            "pages": [{"page": 1}, {"page": 2}]
        }
    })
}

#[tokio::test]
async fn queries_view_data_and_best_effort_followers() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/view"))
        .and(query_param("bvid", "BV1GJ411x7h7"))
        .respond_with(ResponseTemplate::new(200).set_body_json(view_body()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/relation"))
        .and(query_param("vmid", "42"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "code": 0, "data": {"follower": 1234}
        })))
        .mount(&server)
        .await;

    let video = client(&server, Vec::new())
        .query(&BilibiliTarget::Video(VideoId::Bv(
            "BV1GJ411x7h7".to_owned(),
        )))
        .await
        .unwrap();

    assert_eq!(video.title, "Example");
    assert_eq!(video.owner.followers, Some(1234));
    assert_eq!(video.stats.views, 1000);
    assert_eq!(video.pages, 2);
}

#[tokio::test]
async fn follower_failure_does_not_discard_the_video() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/view"))
        .respond_with(ResponseTemplate::new(200).set_body_json(view_body()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/relation"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let video = client(&server, Vec::new())
        .query(&BilibiliTarget::Video(VideoId::Bv(
            "BV1GJ411x7h7".to_owned(),
        )))
        .await
        .unwrap();

    assert_eq!(video.owner.followers, None);
    assert_eq!(video.title, "Example");
}

#[tokio::test]
async fn business_error_is_not_found() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/view"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "code": -404, "data": null
        })))
        .mount(&server)
        .await;

    let error = client(&server, Vec::new())
        .query(&BilibiliTarget::Video(VideoId::Av(1)))
        .await
        .unwrap_err();
    assert!(matches!(error, IntegrationError::NotFound));
}

#[tokio::test]
async fn follows_only_allowed_short_link_redirects() {
    let server = MockServer::start().await;
    let destination = format!("{}/video/BV1GJ411x7h7", server.uri());
    Mock::given(method("GET"))
        .and(path("/short"))
        .respond_with(ResponseTemplate::new(302).insert_header("Location", destination.as_str()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/video/BV1GJ411x7h7"))
        .respond_with(ResponseTemplate::new(200).set_body_string("video page"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/view"))
        .respond_with(ResponseTemplate::new(200).set_body_json(view_body()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/relation"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "code": 0, "data": {"follower": 1}
        })))
        .mount(&server)
        .await;

    let host = Url::parse(&server.uri())
        .unwrap()
        .host_str()
        .unwrap()
        .to_owned();
    let target = Url::parse(&format!("{}/short", server.uri())).unwrap();
    let video = client(&server, vec![host])
        .query(&BilibiliTarget::ShortLink(target))
        .await
        .unwrap();

    assert_eq!(video.id, VideoId::Bv("BV1GJ411x7h7".to_owned()));
}

#[tokio::test]
async fn rejects_disallowed_short_link_hosts_before_network() {
    let server = MockServer::start().await;
    let target = Url::parse(&format!("{}/short", server.uri())).unwrap();
    let error = client(&server, Vec::new())
        .query(&BilibiliTarget::ShortLink(target))
        .await
        .unwrap_err();

    assert!(matches!(error, IntegrationError::InvalidInput { .. }));
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn rejects_redirects_to_disallowed_hosts() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/short"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("Location", "http://example.com/video/BV1GJ411x7h7"),
        )
        .mount(&server)
        .await;

    let host = Url::parse(&server.uri())
        .unwrap()
        .host_str()
        .unwrap()
        .to_owned();
    let target = Url::parse(&format!("{}/short", server.uri())).unwrap();
    let error = client(&server, vec![host])
        .query(&BilibiliTarget::ShortLink(target))
        .await
        .unwrap_err();

    assert!(matches!(error, IntegrationError::InvalidInput { .. }));
}
