# Vendored `rama-dns` fork

Source: [`rama-dns` v0.3.0-alpha.4](https://crates.io/crates/rama-dns/0.3.0-alpha.4)
(MIT OR Apache-2.0, <https://github.com/plabayo/rama>), vendored verbatim except
for the changes listed below and wired in through `[patch.crates-io]` in
`codex-rs/Cargo.toml`.

## Why

`codex-network-proxy` depends on `rama-tcp` v0.3.0-alpha.4, which depends on
`rama-dns` v0.3.0-alpha.4, which pins `hickory-resolver ^0.25`. That drags in
`hickory-proto` v0.25.2, which is affected by:

- **RUSTSEC-2026-0118** — DNSSEC validation flaws (not reachable here; DNSSEC
  features are never enabled).
- **RUSTSEC-2026-0119** / **GHSA-q2qq-hmj6-3wpp** — CPU exhaustion during
  message encoding: `BinEncoder` keeps name-compression label candidates in a
  `Vec` and matches them with a linear scan, so a message with many records
  costs O(n²) to encode.

Both are only fixed in `hickory-proto` 0.26.1; there is no patched 0.25.x
release. Every `rama` release that moved to `hickory-resolver ^0.26.1`
(0.3.0-rc1 and later) also raised its MSRV to rustc 1.96.0 — above this repo's
pinned 1.95.0 toolchain — and removed the `rama-core` APIs that
`codex-network-proxy` is built on (`ExtensionsMut`, `RequestContext`,
`ProxyRequest`, `ProxyTarget`, `TryRefIntoTransportContext`,
`TlsConnectorDataBuilder`). Bumping this one leaf crate is the small change;
the `rama` 0.3.0 upgrade is the large one.

## What changed

`Cargo.toml`

- `hickory-resolver` `0.25` → `0.26.1`, same feature set
  (`default-features = false`, `tokio`, `system-config`).
- Added a `tokio` dev-dependency and a `[[test]]` entry for `tests/smoke.rs`
  (the packaged manifest sets `autotests = false`).

`src/hickory.rs` — ported to the hickory-resolver 0.26 API, no behaviour change:

- `hickory_resolver::Name` → `hickory_resolver::proto::rr::Name`.
- `name_server::TokioConnectionProvider` → `net::runtime::TokioRuntimeProvider`.
- `ResolverConfig::{google,cloudflare,quad9}()` →
  `ResolverConfig::udp_and_tcp(&{GOOGLE,CLOUDFLARE,QUAD9})`.
- `ResolverBuilder::build()` is now fallible. `try_new_system` propagates the
  error; `HickoryDnsBuilder::build` keeps its infallible signature and
  `expect`s, which cannot fire because `build()` only fails when
  hickory-resolver is compiled with a TLS feature (`__tls`) and this crate
  pins `default-features = false` without one.
- The `{txt,ipv4,ipv6}_lookup` helpers now return a generic `Lookup` rather
  than a record-typed iterator, so results are read off `Lookup::answers()`
  and filtered by `RData` variant.

`tests/smoke.rs` is new: `#[ignore]`d live-DNS checks for A, AAAA, TXT and the
system-config default resolver. Run them with:

```sh
cargo test --manifest-path vendor/rama-dns/Cargo.toml --test smoke -- --ignored
```

## How to remove this

Delete `vendor/rama-dns`, drop the `rama-dns` entry from `[patch.crates-io]`
and the `exclude` entry from `[workspace]` in `codex-rs/Cargo.toml`. Do this
once `codex-network-proxy` moves to `rama` 0.3.0 or later, where `rama-tcp` no
longer depends on `rama-dns` at all and `rama-dns` makes `hickory-resolver` an
optional `hickory` feature. That upgrade also requires raising the repo's
pinned rustc from 1.95.0 to at least 1.96.0.
