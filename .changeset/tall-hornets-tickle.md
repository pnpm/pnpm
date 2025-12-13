---
"@pnpm/plugin-commands-publishing": patch
---

pnpm uses npm under the hood when publishing, and npm doesn't support the `:-fallback` syntax, resulting in npm not reading any envs specified with a fallback. For example, if the npmrc contains `//registry.npmjs.org/:_authToken=${NODE_AUTH_TOKEN-fallback}`, all publish commands fails even if the NODE_AUTH_TOKEN env is set. The fallbacks are now stripped before publishing, i.e. the line becomes `//registry.npmjs.org/:_authToken=${NODE_AUTH_TOKEN}`.
