---
"@pnpm/installing.deps-restorer": minor
"@pnpm/deps.graph-builder": minor
"@pnpm/lockfile.filtering": minor
"@pnpm/config.package-is-installable": minor
"pnpm": minor
---

Fixed an installed optional dependency being left without one of its own required dependencies. When a package reached through `optionalDependencies` is installable on the current system but one of its regular `dependencies` is not, a lockfile-based install skipped that dependency and installed the parent anyway, so importing the parent failed with `MODULE_NOT_FOUND`. The dependency is now installed, and an install-check warning reports the incompatibility. A dependency is still only skipped when every path to it is optional, or when the package that pulls it in was itself skipped [#13286](https://github.com/pnpm/pnpm/issues/13286).
