---
"@pnpm/pnpr": minor
---

pnpr now serves npm under `/npm/` even when npm is the only ecosystem it serves. Point clients at `https://<pnpr>/npm/` and at `https://<pnpr>/npm/~<name>/` for a named registry. Cargo, Python, and container image registries always use their `/cargo/`, `/pypi/`, and `/oci/` prefixes too. The default container image registry still answers at `/v2/` on the host root.

`/metrics` now always returns the server's Prometheus metrics. The npm package named `metrics` is at `/npm/metrics`.
