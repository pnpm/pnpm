---
"pacquet": patch
---

A frozen install now fails when `autoInstallPeers`, `dedupePeers`, or `excludeLinksFromLockfile` has changed since `pnpm-lock.yaml` was written, instead of installing against a lockfile that no longer matches the settings. The error names the drifted setting, as `pnpm install --frozen-lockfile` has always done.
