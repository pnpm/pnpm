---
"@pnpm/lockfile.utils": patch
"pnpm": patch
---

Git dependencies that point to a subdirectory of a repository (`repo#commit&path:/sub/dir`) keep their `path` in the lockfile again. Since the integrity of git-hosted tarballs started being pinned in the lockfile, any install that actually downloaded the tarball rebuilt the lockfile resolution as `{ integrity, tarball, gitHosted }` and dropped the `path` field, while installs served from the store kept it — so the field disappeared seemingly at random. Without `path`, later installs from that lockfile silently unpacked the repository root instead of the subdirectory [#12304](https://github.com/pnpm/pnpm/issues/12304).
