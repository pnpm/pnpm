---
"pacquet": patch
---

The `Request took` warning for package metadata now starts timing when pnpm sends the request. Previously, it also counted the time the request waited for one of pnpm's concurrent request slots, so large installs printed it for requests the registry answered quickly.
