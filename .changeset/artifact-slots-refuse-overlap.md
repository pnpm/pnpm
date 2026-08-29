---
"@pnpm/pnpr": patch
---

A published build artifact is now immutable for every consumer it can reach, not only for the exact compatibility constraints it declares. Publishing an artifact whose constraints overlap one already published for the same input key answers `409 Conflict`, so a later universal, broader, or higher-floor build can no longer take precedence over the artifact a consumer already receives. An entry that already holds artifacts reaching one consumer refuses further publication, including republication of either, so the state is reported rather than hidden. Publishing across a platform matrix is unaffected: artifacts for different operating systems, architectures, or Node majors share no consumer and coexist as before.
