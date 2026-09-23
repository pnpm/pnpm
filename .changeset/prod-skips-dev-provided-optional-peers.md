---
"@pnpm/building.after-install": patch
"@pnpm/deps.compliance.audit": patch
"@pnpm/deps.compliance.commands": patch
"@pnpm/deps.compliance.license-scanner": patch
"@pnpm/deps.compliance.sbom": patch
"@pnpm/deps.inspection.commands": patch
"@pnpm/deps.inspection.list": patch
"@pnpm/deps.inspection.tree-builder": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-restorer": patch
"@pnpm/installing.linking.modules-cleaner": patch
"@pnpm/lockfile.detect-dep-types": patch
"@pnpm/lockfile.filtering": patch
"@pnpm/lockfile.peer-edges": patch
"@pnpm/lockfile.walker": patch
"@pnpm/releasing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install --prod`, `pnpm fetch --prod` and `pnpm deploy --prod` no longer install a devDependency that is only there to satisfy an optional peer dependency of a production dependency. `pnpm list`, `pnpm why`, `pnpm licenses`, `pnpm sbom` and `pnpm audit` leave it out of `--prod` results too. The same applies to `--dev`. A peer that is not optional is still installed and audited [#15344](https://github.com/pnpm/pnpm/issues/15344).
