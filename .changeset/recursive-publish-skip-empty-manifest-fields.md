---
"pacquet": patch
---

`pnpm -r publish` now skips workspace manifests whose `name` or `version` field is an empty string.
