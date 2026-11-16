# SQLx SQLite compatibility patch

Author: jaswtao · Date: 2026-11-16

This directory contains the published `sqlx-sqlite` 0.8.6 crate from
https://crates.io/crates/sqlx-sqlite/0.8.6. The upstream revision is recorded in
`.cargo_vcs_info.json`; the original MIT and Apache-2.0 licenses are retained.

Only the `libsqlite3-sys` requirement in `Cargo.toml` and `Cargo.toml.orig` is
changed from `0.30.1` to `0.39.1`. Driver source files are unmodified.
This bundles SQLite 3.54.1 to address
[RUSTSEC-2026-0459](https://rustsec.org/advisories/RUSTSEC-2026-0459), while
preserving the application's SQLx 0.8 API and Rust 1.90 baseline.

Remove this patch when an upstream SQLx release supports a fixed SQLite version.
Validate it with the workspace storage/migration tests, the minimum-Rust check,
and the dependency audit. The patched driver is included in the release source
archive and third-party license notices.
