---
"pacquet": patch
---

Fixed several bugs in applying and committing patches:

- A patch that only changes a file's mode no longer corrupts a binary file.
- A patch with CRLF line endings now applies when a blank context line has lost its leading space.
- A unified diff without `diff --git` lines now applies.
- A patch that creates a symlink now fails with `ERR_PNPM_INVALID_PATCH`. Previously pnpm wrote the symlink as a regular file.
- `pnpm patch-commit --patches-dir ../outside` now fails with `ERR_PNPM_PATCHES_DIR_OUTSIDE_PROJECT`. Previously pnpm wrote the patch to `outside` inside the project.
