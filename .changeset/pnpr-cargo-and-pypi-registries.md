---
"@pnpm/pnpr": minor
---

pnpr can now serve Cargo and Python registries beside npm ones. A registry declared with `ecosystem: cargo` speaks the Cargo sparse-index and crates API at its `/~<name>/` endpoint, so `cargo publish`, `cargo yank`, and crate downloads work against a hosted registry, and an upstream such as `https://index.crates.io/` is proxied with downloads verified against the index checksum. A registry declared with `ecosystem: pypi` speaks the Python Simple Repository API (PEP 503 HTML and PEP 691 JSON) and the legacy upload API, so `pip install --index-url` and `twine upload` work against a hosted registry, and an upstream such as `https://pypi.org/simple/` is proxied with files verified against the page's `sha256`. pnpr accepts the bare `Authorization` token `cargo` sends and the `Basic __token__:<token>` form `twine` and `pip` send.
