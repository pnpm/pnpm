---
"@pnpm/lockfile.fs": patch
"pnpm": patch
"pacquet": patch
---

Fixed `pnpm install` failing with `ERR_PNPM_LOCKFILE_IS_SYMLINK` when `pnpm-lock.yaml` is a symlink — as build sandboxes such as Bazel and Nix stage it — and the project has config dependencies. An install that leaves the env document unchanged no longer rewrites it, and writing a *changed* env document through a symlink is still refused. A lockfile that carries a byte order mark now keeps its main document when the env document is replaced.
