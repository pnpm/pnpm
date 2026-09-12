---
"@pnpm/pnpr": patch
---

A published build artifact is now immutable: one input key and one set of compatibility constraints admit one artifact, so publishing a different one over it answers `409 Conflict`, the same as a re-published `name@version`. Republishing the identical artifact still succeeds. Artifacts already stored by an earlier version keep their slot, so upgrading a populated registry does not leave them replaceable.
