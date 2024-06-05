---
"@pnpm/cli-utils": patch
"pnpm": patch
---

pnpm doesn't fail if its version doesn't match the one specified in the "packageManager" field of `package.json` [#8087](https://github.com/pnpm/pnpm/issues/8087).
