---
"@pnpm/types": minor
"@pnpm/installing.commands": patch
"@pnpm/global.commands": patch
"pnpm": minor
"pacquet": minor
---

The `save-prefix` setting now accepts `=`: newly added dependencies are saved with an explicit `=` operator (`=1.2.3`) instead of the setting being silently treated as the default `^`.
