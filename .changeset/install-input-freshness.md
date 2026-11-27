---
"pnpm": patch
---

Fixed filtered dependency checks accepting packages installed from a previous registry after a frozen install. Exclusion pruning now preserves negated name patterns and patterns containing `?`.
