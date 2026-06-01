---
"@pnpm/deps.compliance.commands": minor
"@pnpm/deps.compliance.sbom": minor
"pnpm": minor
---

Improved monorepo SBOM generation. When `--filter` selects a single workspace, the root component now uses that workspace's metadata instead of the workspace root's. Workspace inter-dependencies (`workspace:` protocol) and their transitive dependencies are now included in the SBOM output. Author, repository, and license fall back to the root manifest when the workspace package doesn't define them.
