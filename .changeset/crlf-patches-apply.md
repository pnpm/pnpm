---
"pacquet": patch
---

Patches saved with CRLF line endings now apply, including a patch that creates or deletes a file. pnpm used to reject the git headers of such a patch with `ERR_PNPM_INVALID_PATCH` and the message `invalid file mode: 100644` [#16641](https://github.com/pnpm/pnpm/issues/16641).
