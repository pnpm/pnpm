---
"@pnpm/engine.pm.commands": patch
"pnpm": patch
---

`pnpm self-update` now honors `trustPolicy=no-downgrade`. It resolves the target pnpm version against full registry metadata, so it refuses to switch to a version whose supply-chain trust evidence is weaker than an earlier-published one, the same way a regular install does.
