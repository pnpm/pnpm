---
"pacquet": minor
---

With `lockfile.format: v13`, `pnpm install` records the dependency specifiers of local directory packages. Frozen installs reject changed specifiers even when the locked versions still satisfy them.
