---
"@pnpm/pnpr": minor
---

On npm registries, a `packages:` entry without a `publish` list now admits only the package's maintainers. Before, any signed-in account could publish a new version of any package and move its dist-tags. Set `publish: $authenticated` to keep the old behavior. Cargo, PyPI, and OCI registries keep `$authenticated` as the default and reject `$owners`, because they do not record package maintainers.
