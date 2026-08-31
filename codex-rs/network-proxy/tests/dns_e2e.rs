//! End-to-end checks that the proxy resolves real hostnames and still enforces
//! its allowlist. These exercise the DNS path that goes through the vendored
//! `rama-dns` fork (see `codex-rs/vendor/rama-dns/PATCH.md`), so they are the
//! regression test for that hand-ported hickory-resolver 0.26 adapter.
//!
//! They speak HTTP/1.1 to the proxy over a raw socket rather than pulling in an
//! HTTP client: it keeps the crate's dependency surface unchanged, and it lets
//! the blocked case assert the proxy's actual 403 instead of "some error".
//!
//! They talk to the public internet, so they are `#[ignore]`d by default:
//!
//! ```text
//! cargo test -p codex-network-proxy --test dns_e2e -- --ignored --nocapture
//! ```

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use codex_network_proxy::ConfigReloader;
use codex_network_proxy::ConfigState;
use codex_network_proxy::NetworkProxy;
use codex_network_proxy::NetworkProxyConfig;
use codex_network_proxy::NetworkProxyConstraints;
use codex_network_proxy::NetworkProxyHandle;
use codex_network_proxy::NetworkProxyState;
use codex_network_proxy::build_config_state;
use pretty_assertions::assert_eq;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

const ALLOWED_HOST: &str = "example.com";
const BLOCKED_HOST: &str = "www.rust-lang.org";

struct NoopReloader;

#[async_trait]
impl ConfigReloader for NoopReloader {
    fn source_label(&self) -> String {
        "dns_e2e test config".to_string()
    }

    async fn maybe_reload(&self) -> Result<Option<ConfigState>> {
        Ok(None)
    }

    async fn reload_now(&self) -> Result<ConfigState> {
        Err(anyhow::anyhow!(
            "force reload is not supported in this test"
        ))
    }
}

fn state_allowing(domains: &[&str]) -> Result<Arc<NetworkProxyState>> {
    let permissions: serde_json::Map<String, serde_json::Value> = domains
        .iter()
        .map(|domain| ((*domain).to_string(), serde_json::json!("allow")))
        .collect();
    let config: NetworkProxyConfig = serde_json::from_value(serde_json::json!({
        "network": {
            "enabled": true,
            "enable_socks5": false,
            "enable_socks5_udp": false,
            "allow_upstream_proxy": false,
            "allow_local_binding": false,
            "mode": "full",
            "domains": permissions,
        }
    }))?;
    // Guard against a silently-ignored config key leaving the allowlist empty,
    // which would let the "blocked" case pass for the wrong reason.
    assert_eq!(
        config.network.allowed_domains().unwrap_or_default(),
        domains
            .iter()
            .map(|domain| (*domain).to_string())
            .collect::<Vec<_>>(),
        "test config did not parse into the expected allowlist"
    );
    let state = build_config_state(config, NetworkProxyConstraints::default())?;
    Ok(Arc::new(NetworkProxyState::with_reloader(
        state,
        Arc::new(NoopReloader),
    )))
}

/// Read the status line and headers, stopping at the blank line. This must not
/// read to EOF: on a successful CONNECT the proxy keeps the tunnel open.
async fn read_head(stream: &mut TcpStream) -> Result<String> {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    while !buf.ends_with(b"\r\n\r\n") {
        let read = stream.read(&mut byte).await?;
        if read == 0 {
            break;
        }
        buf.push(byte[0]);
        if buf.len() > 64 * 1024 {
            anyhow::bail!("response head exceeded 64 KiB");
        }
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// Cap on the whole request/response exchange, so a proxy that accepts the
/// connection but never completes a response fails the test instead of hanging
/// it forever.
const EXCHANGE_TIMEOUT: Duration = Duration::from_secs(30);

async fn send(addr: SocketAddr, request: &str) -> Result<String> {
    tokio::time::timeout(EXCHANGE_TIMEOUT, async {
        let mut stream = TcpStream::connect(addr)
            .await
            .context("connect to the proxy listener")?;
        stream.write_all(request.as_bytes()).await?;
        stream.flush().await?;
        read_head(&mut stream).await
    })
    .await
    .with_context(|| format!("proxy did not answer within {EXCHANGE_TIMEOUT:?}"))?
}

async fn start_proxy(state: Arc<NetworkProxyState>) -> Result<(SocketAddr, NetworkProxyHandle)> {
    let proxy = NetworkProxy::builder()
        .state(state)
        .http_addr("127.0.0.1:0".parse()?)
        .build()
        .await?;
    let addr = proxy.http_addr();
    let handle = proxy.run().await?;
    Ok((addr, handle))
}

fn status_line(head: &str) -> &str {
    head.lines().next().unwrap_or_default()
}

/// Forward a plain HTTP request for an allowlisted host. The proxy has to
/// resolve the hostname and open an upstream connection to answer, so a real
/// HTTP status line back means the patched hickory adapter answered inside the
/// live proxy path.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires network access"]
async fn proxy_resolves_and_forwards_plain_http_for_allowlisted_host() -> Result<()> {
    let (addr, handle) = start_proxy(state_allowing(&[ALLOWED_HOST])?).await?;

    let head = send(
        addr,
        &format!(
            "GET http://{ALLOWED_HOST}/ HTTP/1.1\r\nHost: {ALLOWED_HOST}\r\nConnection: close\r\n\r\n"
        ),
    )
    .await;

    handle.shutdown().await?;
    let head = head?;

    let status = status_line(&head);
    assert!(
        status.starts_with("HTTP/1.1 2") || status.starts_with("HTTP/1.1 3"),
        "expected an upstream response through the proxy, got {status:?}"
    );
    assert!(
        !head.to_ascii_lowercase().contains("x-proxy-error"),
        "allowlisted host was blocked by the proxy: {head}"
    );
    println!("plain HTTP through proxy: {status}");
    Ok(())
}

/// A CONNECT for an allowlisted host must reach "200 Connection Established",
/// which the proxy can only send after resolving the host and completing the
/// upstream TCP connection. No TLS handshake is needed to prove that.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires network access"]
async fn proxy_establishes_connect_tunnel_for_allowlisted_host() -> Result<()> {
    let (addr, handle) = start_proxy(state_allowing(&[ALLOWED_HOST])?).await?;

    let head = send(
        addr,
        &format!("CONNECT {ALLOWED_HOST}:443 HTTP/1.1\r\nHost: {ALLOWED_HOST}:443\r\n\r\n"),
    )
    .await;

    handle.shutdown().await?;
    let head = head?;

    let status = status_line(&head);
    assert!(
        status.starts_with("HTTP/1.1 200"),
        "expected the CONNECT tunnel to be established, got {status:?}"
    );
    println!("CONNECT tunnel established: {status}");
    Ok(())
}

/// The same proxy must still refuse a host that is not on the allowlist, and
/// refuse it *as a policy decision*: asserting the 403 and the proxy's own
/// `x-proxy-error` header means this cannot pass because of an unrelated
/// network error.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires network access"]
async fn proxy_blocks_host_outside_allowlist() -> Result<()> {
    let (addr, handle) = start_proxy(state_allowing(&[ALLOWED_HOST])?).await?;

    let head = send(
        addr,
        &format!("CONNECT {BLOCKED_HOST}:443 HTTP/1.1\r\nHost: {BLOCKED_HOST}:443\r\n\r\n"),
    )
    .await;

    handle.shutdown().await?;
    let head = head?;

    assert_eq!(
        status_line(&head),
        "HTTP/1.1 403 Forbidden",
        "expected a policy rejection, got head: {head}"
    );
    assert!(
        head.to_ascii_lowercase()
            .contains("x-proxy-error: blocked-by-allowlist"),
        "expected the allowlist block header, got head: {head}"
    );
    println!("non-allowlisted host refused: 403 blocked-by-allowlist");
    Ok(())
}
