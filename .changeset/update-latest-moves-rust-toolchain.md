---
"pnpm": minor
---

`pnpm update --latest` now moves the Rust release pinned in `rust-toolchain.toml` to the newest release of its channel when `cargo.enabled` is `true`. `1.95.0` becomes the newest stable version, and `nightly-2026-01-01` the newest nightly. `pnpm outdated` lists the pinned release as `rust` when a newer one is out. Pass `rust` to select only the toolchain, or `!rust` to leave it out.
