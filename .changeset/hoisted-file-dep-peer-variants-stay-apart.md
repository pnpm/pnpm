---
"@pnpm/installing.deps-restorer": patch
"@pnpm/installing.linking.real-hoist": patch
"pacquet": patch
"pnpm": patch
---

Under `nodeLinker: hoisted`, peer-resolution variants of an injected directory dependency (a `file:` snapshot) are materialized as separate copies again instead of collapsing onto the first-seen variant. Each copy keeps its own peer-resolved dependency set, so a project pinning one peer version no longer resolves another project's variant — Bit root components with conflicting peers across injected copies rely on this.
