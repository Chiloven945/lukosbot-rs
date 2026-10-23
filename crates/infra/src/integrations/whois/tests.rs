use std::net::SocketAddr;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use lukosbot_integrations::{IntegrationError, WhoisService};

use super::*;

async fn spawn_server(
    expected: &'static [u8],
    response: &'static [u8],
) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buffer = [0_u8; 256];
        let read = stream.read(&mut buffer).await.unwrap();
        assert_eq!(&buffer[..read], expected);
        stream.write_all(response).await.unwrap();
    });
    (address, task)
}

#[tokio::test]
async fn invalid_domains_fail_before_network_io() {
    let client = WhoisClient::new(WhoisSettings::default());
    for invalid in ["localhost", "com", "example.com..", "1.1.1.1"] {
        let error = client.query(invalid).await.unwrap_err();
        assert!(
            matches!(error, IntegrationError::InvalidInput { .. }),
            "{invalid}: {error}"
        );
    }
}

#[test]
fn default_settings_are_bounded() {
    let settings = WhoisSettings::default();
    assert!(settings.connect_timeout <= settings.total_timeout);
    assert!(settings.max_response_bytes > 0);
}

#[tokio::test]
async fn resolves_iana_referral_and_queries_the_registry() {
    let (iana, iana_task) = spawn_server(
        b"io\r\n",
        b"% IANA WHOIS server\ndomain: IO\nwhois:          whois.nic.io\n",
    )
    .await;
    let (registry, registry_task) = spawn_server(
        b"example.io\r\n",
        b"Domain Name: example.io\nRegistrar: Example\n",
    )
    .await;

    let client = WhoisClient::with_test_endpoints(
        WhoisSettings {
            connect_timeout: Duration::from_secs(1),
            total_timeout: Duration::from_secs(2),
            max_response_bytes: 1024,
        },
        iana,
        registry,
    );
    let record = client.query("example.io").await.unwrap();
    iana_task.await.unwrap();
    registry_task.await.unwrap();

    assert_eq!(record.server, "whois.nic.io");
    assert!(record.text.contains("Domain Name: example.io"));
    assert!(!record.truncated);
}

#[tokio::test]
async fn no_iana_referral_is_not_treated_as_a_registry_query() {
    let (iana, iana_task) = spawn_server(b"dev\r\n", b"domain: DEV\nwhois:\n").await;

    let client = WhoisClient::with_test_endpoints(
        WhoisSettings {
            connect_timeout: Duration::from_secs(1),
            total_timeout: Duration::from_secs(2),
            max_response_bytes: 1024,
        },
        iana,
        iana,
    );
    let error = client.query("example.dev").await.unwrap_err();
    iana_task.await.unwrap();

    assert!(matches!(error, IntegrationError::NotFound));
}

#[tokio::test]
async fn maps_no_match_responses_to_not_found() {
    let (iana, iana_task) =
        spawn_server(b"dev\r\n", b"domain: DEV\nwhois:          whois.nic.dev\n").await;
    let (registry, registry_task) = spawn_server(
        b"example.dev\r\n",
        b"No match for \"EXAMPLE.DEV\".\n>>> Last update of whois database: now\n",
    )
    .await;

    let client = WhoisClient::with_test_endpoints(
        WhoisSettings {
            connect_timeout: Duration::from_secs(1),
            total_timeout: Duration::from_secs(2),
            max_response_bytes: 1024,
        },
        iana,
        registry,
    );
    let error = client.query("example.dev").await.unwrap_err();
    iana_task.await.unwrap();
    registry_task.await.unwrap();

    assert!(matches!(error, IntegrationError::NotFound));
}
