---
"@pnpm/lockfile.fs": patch
"pnpm": patch
"pacquet": patch
---

Fixed `pnpm install` failing with `ERR_PNPM_LOCKFILE_IS_SYMLINK` when `pnpm-lock.yaml` is a symlink, as build sandboxes such as Bazel and Nix stage it [#13073](https://github.com/pnpm/pnpm/issues/13073). Reading a lockfile through a symlink is allowed again, and an install that leaves the lockfile unchanged no longer rewrites it, so `--frozen-lockfile` no longer needs to write at all. Writing a *changed* lockfile through a symlink is still refused, as that would redirect the write onto the symlink's target.
