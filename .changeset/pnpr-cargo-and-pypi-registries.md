---
"@pnpm/pnpr": minor
---

pnpr can now serve Cargo and Python registries alongside npm from one instance. Each ecosystem has its own URL prefix, `/npm/`, `/cargo/`, or `/pypi/`. Existing npm URLs keep working.

Hosted Cargo registries support `cargo publish`, `cargo yank`, and crate downloads. Hosted Python registries support `pip install --index-url` and `twine upload`. Upstream registries can proxy crates.io and PyPI with checksum-verified downloads. A router can combine sources from all three ecosystems.
