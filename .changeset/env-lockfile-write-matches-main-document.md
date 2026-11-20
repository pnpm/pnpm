---
"@pnpm/lockfile.fs": patch
"pnpm": patch
"pacquet": patch
---

Fixed `pnpm install` failing with `ERR_PNPM_LOCKFILE_IS_SYMLINK` in a project with config dependencies when `pnpm-lock.yaml` is a symlink, as build sandboxes such as Bazel and Nix stage it. pnpm no longer rewrites the lockfile when the recorded config dependencies are unchanged. Writing changed config dependencies through a symlinked lockfile is still refused. A lockfile that starts with a byte order mark now keeps its main document when its config dependencies are updated [#14372](https://github.com/pnpm/pnpm/issues/14372).
