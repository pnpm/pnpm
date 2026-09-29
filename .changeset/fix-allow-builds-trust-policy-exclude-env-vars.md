---
"@pnpm/config.reader": patch
"pnpm": patch
---

Forward `allowBuilds` and `trustPolicyExclude` to child pnpm processes when running scripts [pnpm/pnpm#10988](https://github.com/pnpm/pnpm/issues/10988).
