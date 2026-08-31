---
"@pnpm/lockfile.fs": patch
"pnpm": patch
"pacquet": patch
---

Fixed `pnpm install` failing with `ERR_PNPM_LOCKFILE_IS_SYMLINK` when `pnpm-lock.yaml` is a symlink — as build sandboxes such as Bazel and Nix stage it — and the project has config dependencies. Writing the leading env document that records them now follows the same rules as the rest of the lockfile: an unchanged document is not rewritten at all, so such an install no longer needs to write, and only a *changed* write through a symlink is refused. A lockfile that carries a byte order mark also keeps its main document intact when the env document is replaced.
