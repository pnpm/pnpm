---
"@pnpm/pnpr.client": major
"@pnpm/pnpr": minor
"pacquet": minor
"pnpm": minor
---

Generalized the experimental shared-artifact protocol so candidates and signed payloads identify a discriminated subject. Dependency side effects use package and source-integrity subjects, while workspace tasks use project and task subjects.

This changes shared-artifact request bodies and signed payloads. A pnpr server and its clients have to be on matching versions.
