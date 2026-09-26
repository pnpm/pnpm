## 1100.2.9

### Patch Changes

- Dependencies and executable binaries are now correctly linked and accessible for workspace packages using `publishConfig.directory` and `publishConfig.linkDirectory` [pnpm/pnpm#8338](https://github.com/pnpm/pnpm/issues/8338).

- `pnpm install --prod`, `pnpm fetch --prod` and `pnpm deploy --prod` no longer install a devDependency that is only there to satisfy an optional peer dependency of a production dependency. `pnpm list`, `pnpm why`, `pnpm licenses`, `pnpm sbom` and `pnpm audit` leave it out of `--prod` results too. The same applies to `--dev`. A peer that is not optional is still installed and audited [#15344](https://github.com/pnpm/pnpm/issues/15344).

- Updated dependencies:
  - @pnpm/config.package-is-installable@1100.2.0
  - @pnpm/deps.path@1101.0.4
  - @pnpm/error@1100.2.0
  - @pnpm/lockfile.types@1100.1.3
  - @pnpm/lockfile.utils@1102.1.4
  - @pnpm/lockfile.walker@1100.0.25
  - @pnpm/types@1102.1.1
