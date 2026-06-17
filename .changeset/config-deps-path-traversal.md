---
"@pnpm/installing.env-installer": patch
"pnpm": patch
---

Security: reject config dependency names from the env lockfile (`pnpm-lock.yaml`) that are not valid npm package names. A committed lockfile with a traversal-shaped `configDependencies` name (such as `../../PWNED`) could previously cause `pnpm install` to create symlinks outside `node_modules/.pnpm-config`. The same validation is now applied to the optional subdependency names of config dependencies. See [GHSA-qrv3-253h-g69c](https://github.com/pnpm/pnpm/security/advisories/GHSA-qrv3-253h-g69c).
