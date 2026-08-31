//! End-to-end checks that the proxy resolves real hostnames and still enforces
//! its allowlist. These exercise the DNS path that goes through the vendored
//! `rama-dns` fork (see `codex-rs/vendor/rama-dns/PATCH.md`), so they are the
//! regression test for that hand-ported hickory-resolver 0.26 adapter.
//!
//! They talk to the public internet, so they are `#[ignore]`d by default:
//!
//! ```text
//! cargo test -p codex-network-proxy --test dns_e2e -- --ignored --nocapture
//! ```

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use codex_network_proxy::ConfigReloader;
use codex_network_proxy::ConfigState;
use codex_network_proxy::NetworkProxy;
use codex_network_proxy::NetworkProxyConfig;
use codex_network_proxy::NetworkProxyConstraints;
use codex_network_proxy::NetworkProxyState;
use codex_network_proxy::build_config_state;

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

/// CONNECT through the proxy to an allowlisted host. The proxy has to resolve
/// the hostname to dial it, so a green result means the patched hickory
/// adapter answered inside the real proxy path.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires network access"]
async fn proxy_connects_to_allowlisted_host() -> Result<()> {
    let state = state_allowing(&["example.com"])?;
    let proxy = NetworkProxy::builder()
        .state(state)
        .http_addr("127.0.0.1:0".parse()?)
        .build()
        .await?;
    let http_addr = proxy.http_addr();
    let handle = proxy.run().await?;

    let client = reqwest::Client::builder()
        .proxy(reqwest::Proxy::all(format!("http://{http_addr}"))?)
        .build()?;
    let response = client.get("https://example.com/").send().await?;
    let status = response.status();
    let body = response.text().await?;

    handle.shutdown().await?;

    assert!(
        status.is_success(),
        "expected a successful response through the proxy, got {status}"
    );
    assert!(
        body.contains("Example Domain"),
        "expected the real example.com body, got {} bytes",
        body.len()
    );
    println!("allowlisted host: {status}, {} bytes of body", body.len());
    Ok(())
}

/// The same proxy must still refuse a host that is not on the allowlist.
/// This guards against the DNS change accidentally widening access.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires network access"]
async fn proxy_blocks_host_outside_allowlist() -> Result<()> {
    let state = state_allowing(&["example.com"])?;
    let proxy = NetworkProxy::builder()
        .state(state)
        .http_addr("127.0.0.1:0".parse()?)
        .build()
        .await?;
    let http_addr = proxy.http_addr();
    let handle = proxy.run().await?;

    let client = reqwest::Client::builder()
        .proxy(reqwest::Proxy::all(format!("http://{http_addr}"))?)
        .build()?;
    let result = client.get("https://www.rust-lang.org/").send().await;

    handle.shutdown().await?;

    assert!(
        result.is_err(),
        "expected the proxy to refuse a host outside the allowlist, got {result:?}"
    );
    println!("non-allowlisted host refused as expected");
    Ok(())
}
