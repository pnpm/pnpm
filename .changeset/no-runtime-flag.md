---
"@pnpm/lockfile.filtering": minor
"@pnpm/installing.linking.modules-cleaner": minor
"@pnpm/installing.deps-restorer": minor
"@pnpm/installing.deps-installer": minor
"@pnpm/installing.commands": minor
"@pnpm/config.reader": minor
"pnpm": minor
---

Add `--no-runtime` flag (config: `runtime=false`) to skip installing runtime entries (e.g. Node.js downloaded via `devEngines.runtime`) without modifying the lockfile. The lockfile keeps the runtime entry so frozen-lockfile validation still passes; only the runtime fetch and `.bin` linking are skipped. Useful in CI matrices where the runtime is provisioned externally (e.g. via `pnpm runtime -g set node <version>`) before `pnpm install` runs.
