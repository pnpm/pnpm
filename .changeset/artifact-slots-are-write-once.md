---
"@pnpm/pnpr": patch
---

A published build artifact is now immutable: one input key and one set of compatibility constraints admit one artifact, and publishing a different one over it is refused with `409 Conflict`, the same answer a re-published `name@version` gets. Re-publishing the identical artifact stays idempotent.

Releasing a claimed slot is an operator action against the store rather than something a publishing credential can do, so a stolen token cannot swap the artifact for a dependency nobody has looked at in a year.
