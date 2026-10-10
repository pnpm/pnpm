---
"@pnpm/config.reader": patch
"pnpm": patch
"pacquet": patch
---

A key under a registry URL in the `_auth` setting that does not start with `@` is now skipped with a warning, so a field a later pnpm version adds there no longer breaks this one. A key that holds an `authToken` object but lacks its `@`, such as `org` for `@org`, is still an error.
