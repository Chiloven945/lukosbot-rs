use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use lukosbot_integrations::IntegrationError;

use super::*;

#[test]
fn normalizes_idna_and_trailing_dots() {
    assert_eq!(normalize_domain("Example.COM.").unwrap(), "example.com");
    assert_eq!(
        normalize_domain("例子.中国").unwrap(),
        "xn--fsqu00a.xn--fiqs8s"
    );
    assert_eq!(static_server("cn"), Some("whois.cnnic.cn"));
    assert_eq!(static_server("xn--fiqs8s"), Some("whois.cnnic.cn"));
    assert_eq!(static_server("com"), Some("whois.internic.net"));
}

#[test]
fn rejects_injection_and_invalid_domains() {
    for invalid in [
        "",
        "example.com\r\n",
        "example.com:43",
        "https://example.com",
        "1.1.1.1",
        "-bad.example",
        "bad-.example",
        "a..b",
    ] {
        assert!(normalize_domain(invalid).is_err(), "{invalid}");
    }
}

#[test]
fn parses_iana_referrals_only_from_valid_hosts() {
    assert_eq!(
        parse_iana_referral("domain: TEST\nwhois: whois.example.test\n"),
        Some("whois.example.test".to_owned())
    );
    assert_eq!(parse_iana_referral("whois: 127.0.0.1\n"), None);
}

#[test]
fn cleans_boilerplate_duplicates_and_blank_runs() {
    let raw = "Domain: EXAMPLE.COM\r\n\
               Registrar: One\r\n\
               Empty:\r\n\
               Registrar: Two\r\n\
               \r\n\r\n\r\n\
               NOTICE: stop here\r\n\
               After: ignored\r\n";
    let (text, _) = clean_whois(raw);
    assert!(text.contains("Registrar: One, Two"));
    assert!(!text.contains("Empty:"));
    assert!(!text.contains("NOTICE"));
    assert!(!text.contains("After:"));
    assert!(text.starts_with("Domain: EXAMPLE.COM"));
}

#[tokio::test]
async fn exchanges_a_bounded_whois_query_with_a_mock_server() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0_u8; 128];
        let read = socket.read(&mut request).await.unwrap();
        assert_eq!(&request[..read], b"example.com\r\n");
        socket.write_all(b"Domain: EXAMPLE.COM\r\n").await.unwrap();
    });

    let deadline = Instant::now() + Duration::from_secs(2);
    let (text, truncated) = exchange_addr(
        address,
        "example.com",
        deadline,
        Duration::from_secs(1),
        1024,
    )
    .await
    .unwrap();
    server.await.unwrap();

    assert_eq!(text.trim(), "Domain: EXAMPLE.COM");
    assert!(!truncated);
}

#[tokio::test]
async fn exchange_enforces_the_response_limit() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0_u8; 128];
        let _ = socket.read(&mut request).await.unwrap();
        socket.write_all(&[b'x'; 100]).await.unwrap();
    });

    let deadline = Instant::now() + Duration::from_secs(2);
    let (text, truncated) =
        exchange_addr(address, "example.com", deadline, Duration::from_secs(1), 10)
            .await
            .unwrap();
    server.await.unwrap();

    assert_eq!(text, "xxxxxxxxxx");
    assert!(truncated);
}

#[test]
fn rejects_empty_domain_with_a_typed_error() {
    assert!(matches!(
        normalize_domain(""),
        Err(IntegrationError::InvalidInput { .. })
    ));
}

#[test]
fn final_output_is_character_limited() {
    let raw = format!("Key: {}", "值".repeat(MAX_DISPLAY_CHARS + 100));
    let (text, truncated) = clean_whois(&raw);
    assert!(truncated);
    assert_eq!(text.chars().count(), MAX_DISPLAY_CHARS);
}
