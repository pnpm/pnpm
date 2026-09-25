---
"@pnpm/engine.pm.commands": patch
"pacquet": patch
"pnpm": patch
---

When `minimumReleaseAge` holds `latest` back, `pnpm self-update` says the registry's latest release is still within the cutoff. It does not suggest a downgrade [#12006](https://github.com/pnpm/pnpm/issues/12006).
