---
"@pnpm/building.after-install": major
"@pnpm/building.commands": major
"@pnpm/deps.inspection.tree-builder": major
"@pnpm/installing.context": major
"@pnpm/installing.deps-installer": major
"@pnpm/installing.deps-restorer": major
"@pnpm/installing.modules-yaml": major
"@pnpm/installing.read-projects-context": major
"pacquet": minor
"pnpm": minor
---

`node_modules/.modules.yaml` no longer records the registries an install resolved from, and the recorded copy is dropped from the file on the first install that rewrites it.

It dated from the lockfile format that spelled a dependency's path relative to its registry, where reading an installed tree meant knowing the registries it was installed with. Dependency paths have not carried a registry for several major versions, and the recorded copy outlived its use: `pnpm list`, `pnpm why`, and single-project installs preferred it over the project's own configuration, so a project whose registry had changed since its last install was still read through the old one.

They now use the configured registries, like every other command already did.
