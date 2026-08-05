---
"pacquet": patch
---

`patchedDependencies` patch files that pnpm applies no longer fail with `ERR_PNPM_PATCH_FAILED`: a hunk whose last line is context in a file with no final newline, and an LF patch against a CRLF file, both apply again [#13322](https://github.com/pnpm/pnpm/issues/13322). A hunk that has drifted from its recorded line numbers is also retried nearby, matching pnpm.
