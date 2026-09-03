---
"@pnpm/pnpr": patch
---

A published build artifact is now immutable for every consumer it can reach, not only for the exact compatibility constraints it declares. Publishing an artifact that reaches a machine an existing one already reaches answers `409 Conflict`, so a later universal, broader, or higher-floor build can no longer take precedence over the artifact a consumer already receives. Publishing across a platform matrix is unaffected: artifacts for different operating systems, architectures, or Node majors reach no machine in common and coexist as before.
