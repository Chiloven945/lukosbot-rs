use std::time::Duration;

use lukosbot_infra::http::{HttpClient, HttpError, HttpSettings, RedirectMode, RetryPolicy};
use serde_json::json;
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(max_response_bytes: u64, request_timeout: Duration) -> HttpClient {
    HttpClient::new(HttpSettings {
        connect_timeout: Duration::from_secs(2),
        request_timeout,
        max_response_bytes,
        user_agent: "LukosBot-tests/0.1".to_owned(),
        proxy: None,
    })
    .expect("HTTP client builds")
}

fn client_with_redirect(mode: RedirectMode) -> HttpClient {
    HttpClient::new_with_redirect(
        HttpSettings {
            connect_timeout: Duration::from_secs(2),
            request_timeout: Duration::from_secs(2),
            max_response_bytes: 1024,
            user_agent: "LukosBot-tests/0.1".to_owned(),
            proxy: None,
        },
        mode,
    )
    .expect("HTTP client builds")
}

fn url(server: &MockServer, path: &str) -> Url {
    Url::parse(&format!("{}{path}", server.uri())).expect("mock server URL is valid")
}

#[tokio::test]
async fn decodes_successful_json_responses() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/json"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(r#"{"ok":true}"#)
                .insert_header("content-type", "application/json"),
        )
        .mount(&server)
        .await;

    let value: serde_json::Value = client(1024, Duration::from_secs(2))
        .get_json(url(&server, "/json"))
        .await
        .expect("request succeeds");

    assert_eq!(value, json!({"ok": true}));
}

#[tokio::test]
async fn non_success_statuses_are_typed_and_include_a_snippet() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/missing"))
        .respond_with(ResponseTemplate::new(404).set_body_string("not here"))
        .mount(&server)
        .await;

    let error = client(1024, Duration::from_secs(2))
        .get(url(&server, "/missing"))
        .await
        .expect_err("404 is an error");

    match error {
        HttpError::Status { status, snippet } => {
            assert_eq!(status.as_u16(), 404);
            assert!(snippet.contains("not here"));
        }
        other => panic!("unexpected error: {other}"),
    }
}

#[tokio::test]
async fn invalid_json_is_reported_separately_from_status_errors() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/invalid"))
        .respond_with(ResponseTemplate::new(200).set_body_string("not-json"))
        .mount(&server)
        .await;

    let error = client(1024, Duration::from_secs(2))
        .get_json::<serde_json::Value>(url(&server, "/invalid"))
        .await
        .expect_err("invalid JSON is an error");

    assert!(matches!(error, HttpError::Json(_)));
}

#[tokio::test]
async fn oversized_bodies_are_rejected_while_streaming() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/large"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string("0123456789abcdefghijklmnopqrstuvwxyz"),
        )
        .mount(&server)
        .await;

    let error = client(16, Duration::from_secs(2))
        .get_bytes(url(&server, "/large"))
        .await
        .expect_err("body exceeds the limit");

    assert!(matches!(error, HttpError::BodyTooLarge { limit: 16 }));
}

#[tokio::test]
async fn request_timeouts_are_reported() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/slow"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_millis(500))
                .set_body_string("late"),
        )
        .mount(&server)
        .await;

    let error = client(1024, Duration::from_millis(80))
        .get_bytes(url(&server, "/slow"))
        .await
        .expect_err("request times out");

    assert!(matches!(error, HttpError::Timeout));
}

#[tokio::test]
async fn retry_policy_retries_retryable_failures() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/retry"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/retry"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .expect(1)
        .mount(&server)
        .await;

    let policy = RetryPolicy {
        max_attempts: 3,
        initial_backoff: Duration::from_millis(1),
        max_backoff: Duration::from_millis(2),
    };
    let http = client(1024, Duration::from_secs(2));
    let target = url(&server, "/retry");

    let body = policy
        .run(|| http.get_bytes(target.clone()))
        .await
        .expect("retry succeeds");

    assert_eq!(body, b"ok");
}

#[tokio::test]
async fn retry_policy_does_not_retry_non_retryable_failures() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/bad-request"))
        .respond_with(ResponseTemplate::new(400).set_body_string("bad"))
        .expect(1)
        .mount(&server)
        .await;

    let policy = RetryPolicy {
        max_attempts: 5,
        initial_backoff: Duration::from_millis(1),
        max_backoff: Duration::from_millis(2),
    };
    let http = client(1024, Duration::from_secs(2));
    let target = url(&server, "/bad-request");

    let error = policy
        .run(|| http.get_bytes(target.clone()))
        .await
        .expect_err("400 is not retryable");

    assert!(matches!(error, HttpError::Status { status, .. } if status.as_u16() == 400));
}

#[tokio::test]
async fn invalid_proxy_configuration_is_rejected() {
    let result = HttpClient::new(HttpSettings {
        proxy: Some("http://[invalid".to_owned()),
        ..HttpSettings::default()
    });

    assert!(matches!(result, Err(HttpError::Proxy(_))));
}

#[tokio::test]
async fn execute_returns_non_success_statuses_for_callers_that_inspect_headers() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/forbidden"))
        .respond_with(
            ResponseTemplate::new(403)
                .insert_header("retry-after", "7")
                .set_body_string("body"),
        )
        .mount(&server)
        .await;

    let http = client(1024, Duration::from_secs(2));
    let response = http
        .execute(http.raw().get(url(&server, "/forbidden")))
        .await
        .expect("execute does not reject statuses");
    assert_eq!(response.status().as_u16(), 403);
    assert_eq!(
        response
            .headers()
            .get("retry-after")
            .and_then(|value| value.to_str().ok()),
        Some("7")
    );
    assert_eq!(http.read_limited(response).await.unwrap(), b"body");
}

#[tokio::test]
async fn redirect_mode_none_returns_the_redirect_response() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/redirect"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("location", format!("{}/target", server.uri())),
        )
        .mount(&server)
        .await;

    let http = client_with_redirect(RedirectMode::None);
    let response = http
        .execute(http.raw().get(url(&server, "/redirect")))
        .await
        .unwrap();

    assert_eq!(response.status().as_u16(), 302);
    assert_eq!(
        response
            .headers()
            .get("location")
            .and_then(|value| value.to_str().ok()),
        Some(format!("{}/target", server.uri()).as_str())
    );
}

#[tokio::test]
async fn redirect_mode_follow_reaches_the_target() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/redirect"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("location", format!("{}/target", server.uri())),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/target"))
        .respond_with(ResponseTemplate::new(200).set_body_string("target"))
        .mount(&server)
        .await;

    let http = client_with_redirect(RedirectMode::Follow);
    let response = http
        .execute(http.raw().get(url(&server, "/redirect")))
        .await
        .unwrap();

    assert_eq!(response.status().as_u16(), 200);
}
