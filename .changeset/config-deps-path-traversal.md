---
"@pnpm/installing.env-installer": patch
"pnpm": patch
---

Security: validate config dependency names and versions from the env lockfile (`pnpm-lock.yaml`) before using them to build filesystem paths. A committed lockfile with a traversal-shaped `configDependencies` name (such as `../../PWNED`) or version (such as `../../../PWNED`) could previously cause `pnpm install` to create symlinks or write package files outside `node_modules/.pnpm-config` and the store. Names must now be valid npm package names and versions must be exact semver versions; the same validation is applied to optional subdependencies of config dependencies, and to the legacy workspace-manifest format before any lockfile is written. See [GHSA-qrv3-253h-g69c](https://github.com/pnpm/pnpm/security/advisories/GHSA-qrv3-253h-g69c).
