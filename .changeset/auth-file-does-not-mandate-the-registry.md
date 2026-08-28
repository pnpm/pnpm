---
"@pnpm/config.reader": patch
"pnpm": patch
"pacquet": patch
---

An `_auth` entry in the global config file no longer decides which registry packages come from when something else says. A `registry` or `registries` declared in `pnpm-workspace.yaml` or the global config now wins over the route inferred from a stored credential, which still applies where nothing else declares one. The `pnpm_config__auth` environment variable is unchanged: it stays the way to point a CI runner at a mandated proxy, and still overrides what a repository declares.
