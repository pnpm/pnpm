---
"pacquet": patch
---

`pnpm update --latest` now rewrites `jsr:` dependencies. The manifest keeps the protocol and the range operator it declared, so `jsr:1.0.0` becomes `jsr:2.0.0` and `jsr:@scope/name@^1.0.0` becomes `jsr:@scope/name@^2.0.0`, instead of being left at the old version [#13363](https://github.com/pnpm/pnpm/issues/13363).
