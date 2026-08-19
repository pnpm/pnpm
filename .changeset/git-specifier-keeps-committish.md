---
"@pnpm/resolving.git-resolver": patch
"pnpm": patch
"pacquet": patch
---

A git dependency that names a branch, tag, or version range no longer loses it from the specifier recorded in `package.json`. When a hosted repository resolved over HTTPS without being provably public — a private repository reachable through a credential helper, or a URL carrying its own credentials — `pnpm add owner/repo#develop` wrote back `git+https://github.com/owner/repo.git`, dropping `#develop`. The install itself pinned the right commit, so nothing looked wrong until the next `pnpm update` silently moved the dependency to the default branch [#13999](https://github.com/pnpm/pnpm/issues/13999).
