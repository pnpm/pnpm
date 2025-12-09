---
"@pnpm/resolve-dependencies": minor
"@pnpm/core": minor
"@pnpm/config": minor
"pnpm": minor
---

Added a new setting `blockExoticSubdeps` that prevents the resolution of exotic protocols in transitive dependencies.

When set to `true`, direct dependencies (those listed in your root `package.json`) may still use exotic sources, but all transitive dependencies must be resolved from a trusted source. Trusted sources include the configured registry, local file paths, workspace links, trusted GitHub repositories (node, bun, deno), and custom resolvers.

This helps to secure the dependency supply chain. Packages from trusted sources are considered safer, as they are typically subject to more reliable verification and scanning for malware and vulnerabilities.

**Exotic sources** are dependency locations that bypass the usual trusted resolution process. These protocols are specifically targeted and blocked: Git repositories (`git+ssh://...`) and direct URL links to tarballs (`https://.../package.tgz`).

Related PR: [#10265](https://github.com/pnpm/pnpm/pull/10265).
