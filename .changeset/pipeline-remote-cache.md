---
"pacquet": minor
---

`pnpm pipeline` can share task results between machines through a server that speaks the Turborepo Remote Cache API, such as Vercel Remote Cache. Set `pipelineRemoteCache.url` in `pnpm-workspace.yaml` and a token and signing key in the global config or the environment. Uploads are signed, and pnpm restores only results whose signature matches.
