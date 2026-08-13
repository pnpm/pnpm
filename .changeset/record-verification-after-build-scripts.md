---
"@pnpm/installing.deps-installer": patch
"pacquet": patch
"pnpm": patch
---

The lockfile verification result is now recorded once the install's build phase is over, rather than as soon as the lockfile passes. A dependency's lifecycle script runs inside the install and can write to the same log, so recording last keeps pnpm's own verdict distinguishable from anything a script wrote — which matters for CI setups that cache the log between jobs. An install that fails after verification no longer leaves a verdict behind either.
