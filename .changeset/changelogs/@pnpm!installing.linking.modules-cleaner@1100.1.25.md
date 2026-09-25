## 1100.1.25

### Patch Changes

- `pnpm install --prod`, `pnpm fetch --prod` and `pnpm deploy --prod` no longer install a devDependency that is only there to satisfy an optional peer dependency of a production dependency. `pnpm list`, `pnpm why`, `pnpm licenses`, `pnpm sbom` and `pnpm audit` leave it out of `--prod` results too. The same applies to `--dev`. A peer that is not optional is still installed and audited [#15344](https://github.com/pnpm/pnpm/issues/15344).

- Updated dependencies:
  - @pnpm/bins.remover@1100.0.25
  - @pnpm/core-loggers@1101.0.1
  - @pnpm/deps.path@1101.0.4
  - @pnpm/lockfile.filtering@1100.2.9
  - @pnpm/lockfile.types@1100.1.3
  - @pnpm/lockfile.utils@1102.1.4
  - @pnpm/store.controller-types@1101.3.1
  - @pnpm/types@1102.1.1
