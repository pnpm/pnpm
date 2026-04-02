---
"@pnpm/network.web-auth": minor
"@pnpm/auth.commands": minor
"pnpm": minor
---

During web-based authentication (`pnpm login`, `pnpm publish`), users can now press ENTER to open the authentication URL in their default browser. The background polling continues uninterrupted, so users who prefer to authenticate on their phone can still do so without pressing anything.
