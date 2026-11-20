---
"@pnpm/pnpr": minor
---

pnpr rejects a version published with a provenance statement that does not name each of its archives by its package URL and SHA-512. A platform variant's archive is named by the version's package URL with a `variant` qualifier, which its release record entry now lists as `purl`.
