---
"@pnpm/hooks.read-package-hook": patch
---

Extends the `pnpm.peerDependencyRules.allowedVersions` package.json option to support the
`parent>child` selector syntax. This syntax allows for overriding specific peerDependencies.
