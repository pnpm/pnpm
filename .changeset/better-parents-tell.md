---
"@pnpm/resolve-dependencies": minor
"@pnpm/core": minor
"@pnpm/config": minor
"pnpm": minor
---

Added a new setting `blockExoticSubdeps` that **enforces that subdependencies are only resolved from the registry**.

When set to `true`, direct dependencies (those listed in your root `package.json`) may still use exotic sources, but all transitive dependencies must be resolved from a registry. This helps to secure the dependency supply chain. Packages from the registry are considered safer than packages from exotic sources, as registry packages are typically subject to regular scanning for malware and vulnerabilities.

**Exotic sources** are dependency locations that bypass the configured package registry. These include Git repositories (`git+ssh://...`), direct URL links to tarballs (`https://.../package.tgz`), and local file paths (`file:../local-package`).

Related PR: [#10265](https://github.com/pnpm/pnpm/pull/10265).
