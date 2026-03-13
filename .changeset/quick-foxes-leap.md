---
"@pnpm/store.cafs": major
"@pnpm/store-controller-types": major
"@pnpm/worker": major
"@pnpm/package-requester": major
"@pnpm/npm-resolver": major
"@pnpm/reviewing.dependencies-hierarchy": major
"@pnpm/build-modules": major
"pnpm": major
---

Store the bundled manifest (name, version, bin, engines, scripts, etc.) directly in the package index file, eliminating the need to read `package.json` from the content-addressable store during resolution and installation. This reduces I/O and speeds up repeat installs [#10473](https://github.com/pnpm/pnpm/pull/10473).
