---
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
"pacquet": patch
---

A warm `pnpm install` reuses on-disk package metadata for five minutes when the registry does not send an ETag. Registries that send an ETag, including the public npm registry, still revalidate with a conditional request. `pnpm update` still fetches current metadata [pnpm/pnpm#13976](https://github.com/pnpm/pnpm/issues/13976).
