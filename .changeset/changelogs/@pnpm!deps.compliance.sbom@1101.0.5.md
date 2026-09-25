## 1101.0.5

### Patch Changes

- `pnpm install --prod`, `pnpm fetch --prod` and `pnpm deploy --prod` no longer install a devDependency that is only there to satisfy an optional peer dependency of a production dependency. `pnpm list`, `pnpm why`, `pnpm licenses`, `pnpm sbom` and `pnpm audit` leave it out of `--prod` results too. The same applies to `--dev`. A peer that is not optional is still installed and audited [#15344](https://github.com/pnpm/pnpm/issues/15344).

- Updated dependencies:
  - @pnpm/config.normalize-registries@1101.0.2
  - @pnpm/config.package-is-installable@1100.2.0
  - @pnpm/error@1100.2.0
  - @pnpm/lockfile.detect-dep-types@1100.0.25
  - @pnpm/lockfile.types@1100.1.3
  - @pnpm/lockfile.utils@1102.1.4
  - @pnpm/lockfile.walker@1100.0.25
  - @pnpm/pkg-manifest.reader@1100.0.20
  - @pnpm/resolving.resolver-base@1101.3.1
  - @pnpm/store.index@1100.3.2
  - @pnpm/store.pkg-finder@1100.0.35
  - @pnpm/types@1102.1.1
