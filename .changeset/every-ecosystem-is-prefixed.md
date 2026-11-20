---
"@pnpm/pnpr": minor
---

pnpr now serves npm under `/npm/` even when npm is the only ecosystem it serves. The npm registry URL is `https://<pnpr>/npm/`, and a named registry is at `https://<pnpr>/npm/~<name>/`.

Cargo, Python, and container image registries likewise always sit under their own prefix. The default container image registry still answers at `/v2/` on the host root.

`/metrics` now always returns the server's Prometheus metrics. The npm package named `metrics` is at `/npm/metrics`.
