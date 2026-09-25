---
"@pnpm/engine.pm.commands": patch
"pacquet": patch
"pnpm": patch
---

`pnpm self-update` no longer suggests a downgrade when `minimumReleaseAge` holds back the registry's `latest` release. It now says that release is still within the cutoff [#12006](https://github.com/pnpm/pnpm/issues/12006).
