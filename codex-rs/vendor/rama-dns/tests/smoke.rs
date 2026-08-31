//! Live smoke tests for the hand-ported hickory-resolver 0.26 adapter.
//!
//! These hit real DNS servers, so they are `#[ignore]`d by default. This crate
//! is excluded from the codex-rs workspace, so run them explicitly with:
//!
//! ```text
//! cargo test --manifest-path vendor/rama-dns/Cargo.toml --test smoke -- --ignored
//! ```

use rama_dns::{DnsResolver, HickoryDns};
use rama_net::address::Domain;

#[tokio::test]
#[ignore = "requires network access"]
async fn resolves_ipv4_via_cloudflare() {
    let dns = HickoryDns::builder().build();
    let ips = dns
        .ipv4_lookup(Domain::from_static("one.one.one.one"))
        .await
        .expect("ipv4 lookup");
    assert!(!ips.is_empty(), "expected at least one A record");
    println!("A records: {ips:?}");
}

#[tokio::test]
#[ignore = "requires network access"]
async fn resolves_ipv6_via_cloudflare() {
    let dns = HickoryDns::builder().build();
    let ips = dns
        .ipv6_lookup(Domain::from_static("one.one.one.one"))
        .await
        .expect("ipv6 lookup");
    assert!(!ips.is_empty(), "expected at least one AAAA record");
    println!("AAAA records: {ips:?}");
}

#[tokio::test]
#[ignore = "requires network access"]
async fn resolves_txt_via_cloudflare() {
    let dns = HickoryDns::builder().build();
    let txt = dns
        .txt_lookup(Domain::from_static("cloudflare.com"))
        .await
        .expect("txt lookup");
    assert!(!txt.is_empty(), "expected at least one TXT record");
    println!("TXT record count: {}", txt.len());
}

#[tokio::test]
#[ignore = "requires network access"]
async fn default_system_resolver_works() {
    let dns = HickoryDns::default();
    let ips = dns
        .ipv4_lookup(Domain::from_static("example.com"))
        .await
        .expect("system ipv4 lookup");
    assert!(!ips.is_empty());
    println!("system A records: {ips:?}");
}
