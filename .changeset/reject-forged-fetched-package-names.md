---
"pnpm": patch
---

pnpm now refuses to install a tarball, git, or directory dependency whose `package.json` has an invalid `name`, and fails with `ERR_PNPM_INVALID_DEPENDENCY_NAME`. A name such as `esbuild@0.20.0(` could make the package pass for a registry package in the lockfile and in `allowBuilds`.

Resolution now fails when two packages with different resolutions would share one `packages` entry of the lockfile.
