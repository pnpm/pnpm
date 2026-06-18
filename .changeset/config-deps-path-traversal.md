---
"@pnpm/config.deps-installer": patch
"pnpm": patch
---

Security: validate config dependency names and versions before using them to build filesystem paths. A `pnpm-workspace.yaml` with a traversal-shaped `configDependencies` name (such as `../../PWNED`) or version (such as `../../../PWNED`) could previously cause `pnpm install` to create symlinks or write package files outside `node_modules/.pnpm-config` and the store. Names must now be valid npm package names and versions must be exact semver versions. See [GHSA-qrv3-253h-g69c](https://github.com/pnpm/pnpm/security/advisories/GHSA-qrv3-253h-g69c).
