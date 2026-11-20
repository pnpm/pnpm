---
"@pnpm/pnpr": minor
---

A `packages:` entry without a `publish` list now admits only the package's owners and `auth.admins`. The account that first publishes a package, crate, Python project, or image repository becomes its owner. An npm package's owners are its maintainers. Before, any signed-in account could publish a new version of another account's package and move its dist-tags. Set `publish: $authenticated` to keep the old behavior.
