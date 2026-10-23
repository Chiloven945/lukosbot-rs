use std::sync::Arc;
use std::time::Duration;

use secrecy::SecretString;
use serde_json::json;
use url::Url;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use lukosbot_integrations::{GitHubSearchParams, GitHubService, GitHubSort, IntegrationError};

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

fn client(server: &MockServer, token: Option<SecretString>) -> GitHubClient {
    GitHubClient::with_api_base(
        http(),
        token,
        Url::parse(&format!("{}/", server.uri())).unwrap(),
    )
}

#[tokio::test]
async fn maps_user_and_sends_bearer_token() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/users/octocat"))
        .and(header("accept", "application/vnd.github.v3+json"))
        .and(header("authorization", "Bearer secret"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "login": "octocat",
            "name": "The Octocat",
            "html_url": "https://github.com/octocat",
            "public_repos": 8,
            "followers": 9,
            "following": 10
        })))
        .expect(1)
        .mount(&server)
        .await;

    let user = client(&server, Some(SecretString::from("secret".to_owned())))
        .user("octocat")
        .await
        .unwrap();

    assert_eq!(user.login, "octocat");
    assert_eq!(user.name.as_deref(), Some("The Octocat"));
    assert_eq!(user.public_repos, 8);
}

#[tokio::test]
async fn omits_authorization_without_a_token() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/users/octocat"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "login": "octocat"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let user = client(&server, None).user("octocat").await.unwrap();
    assert_eq!(user.login, "octocat");
}

#[tokio::test]
async fn maps_repository_and_search_parameters() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/owner/repo"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "full_name": "owner/repo",
            "html_url": "https://github.com/owner/repo",
            "language": "Rust",
            "stargazers_count": 12,
            "forks_count": 3,
            "description": "A repository"
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/search/repositories"))
        .and(query_param("q", "rust language:rust"))
        .and(query_param("per_page", "2"))
        .and(query_param("sort", "stars"))
        .and(query_param("order", "desc"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "total_count": 2,
            "items": [
                {"full_name": "a/one", "html_url": "https://github.com/a/one", "stargazers_count": 5},
                {"full_name": "b/two", "stargazers_count": 4},
                {"full_name": "c/three"}
            ]
        })))
        .mount(&server)
        .await;

    let client = client(&server, None);
    let repository = client.repository("owner", "repo").await.unwrap();
    assert_eq!(repository.full_name, "owner/repo");
    assert_eq!(repository.language.as_deref(), Some("Rust"));

    let result = client
        .search(&GitHubSearchParams {
            query: "rust".to_owned(),
            top: 2,
            language: Some("rust".to_owned()),
            sort: Some(GitHubSort::Stars),
            order: Some(lukosbot_integrations::GitHubOrder::Desc),
        })
        .await
        .unwrap();
    assert_eq!(result.total_count, 2);
    assert_eq!(result.items.len(), 2);
}

#[tokio::test]
async fn classifies_statuses_without_echoing_bodies() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/users/missing"))
        .respond_with(ResponseTemplate::new(404).set_body_string("secret upstream body"))
        .mount(&server)
        .await;

    let error = client(&server, None).user("missing").await.unwrap_err();
    assert!(matches!(error, IntegrationError::NotFound));
}

#[tokio::test]
async fn classifies_rate_limit_headers() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/users/limited"))
        .respond_with(
            ResponseTemplate::new(403)
                .insert_header("x-ratelimit-remaining", "0")
                .insert_header("retry-after", "30"),
        )
        .mount(&server)
        .await;

    let error = client(&server, None).user("limited").await.unwrap_err();
    assert!(matches!(
        error,
        IntegrationError::RateLimited { retry_after: Some(duration) }
            if duration == Duration::from_secs(30)
    ));
}

#[tokio::test]
async fn rejects_invalid_search_top_before_network() {
    let server = MockServer::start().await;
    let error = client(&server, None)
        .search(&GitHubSearchParams {
            query: "rust".to_owned(),
            top: 0,
            language: None,
            sort: None,
            order: None,
        })
        .await
        .unwrap_err();

    assert!(matches!(error, IntegrationError::InvalidInput { .. }));
    assert!(server.received_requests().await.unwrap().is_empty());
}
