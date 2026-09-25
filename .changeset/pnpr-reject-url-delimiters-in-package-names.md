---
"@pnpm/pnpr": patch
---

pnpr now rejects a package name that carries `?`, `#`, `%`, whitespace, or a control character. Artifact filenames are held to the same rule. A percent-encoded delimiter let a request read an upstream package under a name the access rules had not checked.
