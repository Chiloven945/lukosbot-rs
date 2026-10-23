use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, lookup_host};
use tokio::time::Instant;

use lukosbot_integrations::{BoxError, IntegrationError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WhoisSettings {
    pub connect_timeout: Duration,
    pub total_timeout: Duration,
    pub max_response_bytes: u64,
}

impl Default for WhoisSettings {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(3),
            total_timeout: Duration::from_secs(8),
            max_response_bytes: 64 * 1024,
        }
    }
}

/// Queries one validated WHOIS server and returns the raw response plus whether it was truncated.
pub(crate) async fn query_server(
    server: &str,
    query: &str,
    deadline: Instant,
    connect_timeout: Duration,
    max_bytes: usize,
) -> Result<(String, bool), IntegrationError> {
    let stream = connect_public(server, deadline, connect_timeout).await?;
    exchange_stream(stream, query, deadline, max_bytes).await
}

async fn connect_public(
    server: &str,
    deadline: Instant,
    connect_timeout: Duration,
) -> Result<TcpStream, IntegrationError> {
    let addresses = resolve_public_addresses(server, deadline).await?;
    connect_addresses(&addresses, deadline, connect_timeout).await
}

/// Resolves a hostname and returns every public address, IPv4 first.
///
/// Trying every address is essential: many registry servers publish both AAAA and A records, and
/// an environment with broken IPv6 connectivity must not lose the query before the IPv4 attempt.
async fn resolve_public_addresses(
    server: &str,
    deadline: Instant,
) -> Result<Vec<SocketAddr>, IntegrationError> {
    let addresses = tokio::time::timeout_at(deadline, lookup_host((server, 43)))
        .await
        .map_err(|_| IntegrationError::Timeout)?
        .map_err(io_transport)?;

    let mut public = Vec::new();
    for address in addresses {
        if is_public_address(address.ip()) && !public.contains(&address) {
            public.push(address);
        }
    }
    public.sort_by_key(|address| u8::from(address.is_ipv6()));

    if public.is_empty() {
        return Err(IntegrationError::Transport {
            source: Box::new(io::Error::other("the WHOIS hostname has no public address"))
                as BoxError,
        });
    }
    Ok(public)
}

async fn connect_addresses(
    addresses: &[SocketAddr],
    deadline: Instant,
    connect_timeout: Duration,
) -> Result<TcpStream, IntegrationError> {
    let mut last_error = None;

    for address in addresses {
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        let attempt_deadline = deadline.min(now + connect_timeout);
        match tokio::time::timeout_at(attempt_deadline, TcpStream::connect(*address)).await {
            Ok(Ok(stream)) => return Ok(stream),
            Ok(Err(error)) => last_error = Some(error),
            Err(_) => {}
        }
    }

    if let Some(error) = last_error {
        Err(io_transport(error))
    } else {
        Err(IntegrationError::Timeout)
    }
}

async fn exchange_stream(
    mut stream: TcpStream,
    query: &str,
    deadline: Instant,
    max_bytes: usize,
) -> Result<(String, bool), IntegrationError> {
    let line = format!("{query}\r\n");
    tokio::time::timeout_at(deadline, stream.write_all(line.as_bytes()))
        .await
        .map_err(|_| IntegrationError::Timeout)?
        .map_err(io_transport)?;

    let mut body = Vec::new();
    let mut buffer = [0_u8; 4096];
    let mut truncated = false;
    loop {
        let read = match tokio::time::timeout_at(deadline, stream.read(&mut buffer)).await {
            Ok(Ok(0)) => break,
            Ok(Ok(read)) => read,
            Ok(Err(error)) => return Err(io_transport(error)),
            Err(_) => return Err(IntegrationError::Timeout),
        };

        let remaining = max_bytes.saturating_sub(body.len());
        if read > remaining {
            body.extend_from_slice(&buffer[..remaining]);
            truncated = true;
            break;
        }
        body.extend_from_slice(&buffer[..read]);
    }

    Ok((String::from_utf8_lossy(&body).into_owned(), truncated))
}

#[cfg(test)]
pub(crate) async fn query_server_at(
    address: SocketAddr,
    query: &str,
    deadline: Instant,
    connect_timeout: Duration,
    max_bytes: usize,
) -> Result<(String, bool), IntegrationError> {
    let stream = connect_addresses(&[address], deadline, connect_timeout).await?;
    exchange_stream(stream, query, deadline, max_bytes).await
}

pub(crate) fn is_public_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_public_v4(ip),
        IpAddr::V6(ip) => match ip.to_ipv4_mapped() {
            Some(ip) => is_public_v4(ip),
            None => is_public_v6(ip),
        },
    }
}

fn is_public_v4(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    let shared = octets[0] == 100 && (64..=127).contains(&octets[1]);
    let reserved = octets[0] == 0 || octets[0] >= 240;
    !(ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_unspecified()
        || ip.is_multicast()
        || shared
        || reserved)
}

fn is_public_v6(ip: Ipv6Addr) -> bool {
    let segments = ip.segments();
    let documentation = segments[0] == 0x2001 && segments[1] == 0x0db8;
    let site_local = (segments[0] & 0xffc0) == 0xfec0;
    !(ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || ip.is_unique_local()
        || ip.is_unicast_link_local()
        || documentation
        || site_local)
}

fn io_transport(error: io::Error) -> IntegrationError {
    IntegrationError::Transport {
        source: Box::new(error) as BoxError,
    }
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, Ipv6Addr};

    use super::*;

    #[test]
    fn rejects_private_and_mapped_addresses() {
        assert!(!is_public_address(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))));
        assert!(!is_public_address(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))));
        assert!(!is_public_address(IpAddr::V4(Ipv4Addr::new(100, 64, 0, 1))));
        assert!(is_public_address(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1))));
        assert!(!is_public_address(IpAddr::V6(
            "::ffff:127.0.0.1".parse::<Ipv6Addr>().unwrap()
        )));
        assert!(!is_public_address(IpAddr::V6(
            "::ffff:10.0.0.1".parse::<Ipv6Addr>().unwrap()
        )));
        assert!(is_public_address(IpAddr::V6(
            "2606:4700:4700::1111".parse::<Ipv6Addr>().unwrap()
        )));
    }

    #[test]
    fn rejects_documentation_and_link_local_v6() {
        assert!(!is_public_address(IpAddr::V6(
            "2001:db8::1".parse::<Ipv6Addr>().unwrap()
        )));
        assert!(!is_public_address(IpAddr::V6(
            "fe80::1".parse::<Ipv6Addr>().unwrap()
        )));
    }

    #[tokio::test]
    async fn tries_the_next_address_after_a_failed_connection() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let reachable = listener.local_addr().unwrap();
        let closed_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let closed = closed_listener.local_addr().unwrap();
        drop(closed_listener);

        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buffer = [0_u8; 64];
            let read = stream.read(&mut buffer).await.unwrap();
            assert_eq!(&buffer[..read], b"example.com\r\n");
            stream.write_all(b"Domain: EXAMPLE.COM\r\n").await.unwrap();
        });

        let deadline = Instant::now() + Duration::from_secs(2);
        let stream = connect_addresses(&[closed, reachable], deadline, Duration::from_millis(200))
            .await
            .unwrap();
        let (text, truncated) = exchange_stream(stream, "example.com", deadline, 1024)
            .await
            .unwrap();
        server.await.unwrap();

        assert_eq!(text.trim(), "Domain: EXAMPLE.COM");
        assert!(!truncated);
    }
}
