---
"pacquet": minor
---

Added the `disallowPrivateProdDeps` setting. When it is `true`, `pnpm install` fails if a publishable workspace project lists a private workspace project in `dependencies` [pnpm/pnpm#2321](https://github.com/pnpm/pnpm/issues/2321).
