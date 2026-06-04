---
"@pnpm/deps.compliance.commands": minor
"@pnpm/deps.compliance.sbom": minor
"pnpm": minor
---

Added per-package SBOM generation with `--out` and `--split` flags. Use `--out sboms/%s.cdx.json` to write one SBOM per workspace package to individual files, or `--split` for NDJSON output to stdout. When `--filter` selects a single package, the SBOM root component now uses that package's metadata. Workspace inter-dependencies (`workspace:` protocol) and their transitive dependencies are included. Author, repository, and license fall back to the root manifest when the package doesn't define them.
