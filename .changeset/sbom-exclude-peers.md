---
"@pnpm/deps.compliance.sbom": minor
"@pnpm/deps.compliance.commands": minor
"pnpm": minor
---

Added `--exclude-peers` to `pnpm sbom`. With `auto-install-peers` (the default), peer dependencies resolve into the lockfile and are otherwise indistinguishable from the package's own dependencies. The flag drops peer dependencies (and any transitive subtree reachable only through them) from the SBOM. CycloneDX 1.7 has no scope or relationship that expresses "consumer-provided peer", so omission is the only spec-clean handling. The flag name matches `pnpm list --exclude-peers`; note the SBOM flag prunes a peer's exclusive subtree, which is stricter than `pnpm list` (which only hides leaf peers).
