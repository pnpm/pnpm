---
"@pnpm/pkg-manifest.utils": patch
"pacquet": minor
"pnpm": patch
---

Completed pnpm runtime installation parity for Node.js, Deno, and Bun, including runtime failure policy, target architecture selection, and dependency runtime engines. Runtime failure overrides now preserve explicit runtime dependencies without matching engine entries.
