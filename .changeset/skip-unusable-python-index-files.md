---
"pacquet": patch
"@pnpm/pnpr": patch
---

`pnpm install` no longer fails when a Python index lists a file pnpm cannot use, such as a release with no SHA-256 digest or an unreadable wheel filename. That file is left out and the project resolves against the remaining releases.
