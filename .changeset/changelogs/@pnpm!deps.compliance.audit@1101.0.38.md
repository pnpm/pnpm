## 1101.0.38

### Patch Changes

- `pnpm audit` now lists at least one dependency path from every workspace project that depends on a vulnerable package. Before, a project whose dependency was reached through more than 100 paths filled the path list, and other projects that depend on the same package were left out [#12200](https://github.com/pnpm/pnpm/issues/12200).

- `pnpm install --prod`, `pnpm fetch --prod` and `pnpm deploy --prod` no longer install a devDependency that is only there to satisfy an optional peer dependency of a production dependency. `pnpm list`, `pnpm why`, `pnpm licenses`, `pnpm sbom` and `pnpm audit` leave it out of `--prod` results too. The same applies to `--dev`. A peer that is not optional is still installed and audited [#15344](https://github.com/pnpm/pnpm/issues/15344).

- Updated dependencies:
  - @pnpm/deps.path@1101.0.4
  - @pnpm/error@1100.2.0
  - @pnpm/lockfile.detect-dep-types@1100.0.25
  - @pnpm/lockfile.fs@1100.2.9
  - @pnpm/lockfile.types@1100.1.3
  - @pnpm/lockfile.utils@1102.1.4
  - @pnpm/lockfile.walker@1100.0.25
  - @pnpm/network.fetch@1100.1.18
  - @pnpm/types@1102.1.1
