use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use lukosbot_integrations::{IntegrationError, IpLookupService, IpProviderId};

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

fn client(server: &MockServer) -> IpLookupClient {
    IpLookupClient::with_endpoints(
        http(),
        Url::parse(&format!("{}/", server.uri())).unwrap(),
        Url::parse(&format!("{}/", server.uri())).unwrap(),
    )
}

#[tokio::test]
async fn maps_both_provider_shapes() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/1.1.1.1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ip": "1.1.1.1",
            "isp": {"asn": "AS13335", "org": "Cloudflare", "isp": "Cloudflare"},
            "location": {
                "country": "Australia", "country_code": "AU",
                "state": "Queensland", "city": "Brisbane",
                "zipcode": "4000", "latitude": -27.4, "longitude": 153.0,
                "timezone": "Australia/Brisbane"
            },
            "risk": {"is_vpn": true, "risk_score": 10}
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/geoip/1.1.1.1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ip": "1.1.1.1", "country": "Australia", "country_code": "AU",
            "region": "Queensland", "city": "Brisbane", "asn": "AS13335",
            "organization": "Cloudflare"
        })))
        .mount(&server)
        .await;

    let result = client(&server)
        .query("1.1.1.1".parse::<IpAddr>().unwrap(), &[])
        .await
        .unwrap();

    assert_eq!(result.provider, IpProviderId::IpQuery);
    assert_eq!(result.data.city.as_deref(), Some("Brisbane"));
    assert_eq!(result.data.risk.as_ref().unwrap().vpn, Some(true));
}

#[tokio::test]
async fn falls_back_in_user_order() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/1.1.1.1"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/geoip/1.1.1.1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ip": "1.1.1.1", "country": "Australia"
        })))
        .mount(&server)
        .await;

    let result = client(&server)
        .query(
            "1.1.1.1".parse::<IpAddr>().unwrap(),
            &[IpProviderId::IpQuery, IpProviderId::IpSb],
        )
        .await
        .unwrap();

    assert_eq!(result.provider, IpProviderId::IpSb);
    assert_eq!(result.failed_providers, [IpProviderId::IpQuery]);
}

#[tokio::test]
async fn reports_all_attempted_providers() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/1.1.1.1"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/geoip/1.1.1.1"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let error = client(&server)
        .query(
            "1.1.1.1".parse::<IpAddr>().unwrap(),
            &[IpProviderId::IpQuery, IpProviderId::IpSb],
        )
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        IntegrationError::AllProvidersFailed { providers }
            if providers == [IpProviderId::IpQuery, IpProviderId::IpSb]
    ));
}
