---
"@pnpm/deps.compliance.sbom": patch
"pnpm": patch
---

Populate download location for git-sourced dependencies in SBOM output. Previously `pnpm sbom` emitted `NOASSERTION` (SPDX) and omitted the distribution reference (CycloneDX) for git dependencies. Now emits the git URL with commit hash, e.g. `git+https://github.com/user/repo.git#commit`.
