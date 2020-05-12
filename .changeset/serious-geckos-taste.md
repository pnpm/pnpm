---
"supi": minor
---

The `linkWorkspacePackages` option is removed. A new option called `linkWorkspacePackagesDepth` is added.
When `linkWorkspacePackageDepth` is `0`, workspace packages are linked to direct dependencies even if these direct
dependencies are not using workspace ranges (so this is similar to the old `linkWorkspacePackages=true`).
`linkWorkspacePackageDepth` also allows to link workspace packages to subdependencies by setting the max depth.
Setting it to `Infinity` will make the resolution algorithm always prefer packages from the workspace over packages
from the registry.
