## 1104.2.4

### Patch Changes

- Cached metadata for a package published within `minimumReleaseAge` is now revalidated with its ETag, so the npm registry can answer `304 Not Modified`. Before, the next install that checked the cache downloaded the whole document again.

- pnpm no longer downloads every packument again on each install from a registry whose metadata responses forbid caching, such as `Cache-Control: no-store`. pnpm revalidates the cached metadata with a conditional request, so a registry that supports conditional requests answers with a 304 when the package has not changed [#16528](https://github.com/pnpm/pnpm/issues/16528).

- Updated dependencies:
  - @pnpm/config.version-policy@1100.2.6
  - @pnpm/core-loggers@1101.0.2
  - @pnpm/pkg-manifest.utils@1100.4.8
  - @pnpm/resolving.registry.pkg-metadata-filter@1100.0.22
  - @pnpm/resolving.registry.types@1100.2.3
  - @pnpm/store.cafs@1100.3.6
