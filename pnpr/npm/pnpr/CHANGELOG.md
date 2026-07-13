# @pnpm/pnpr

## 0.1.0-alpha.2

### Patch Changes

- Bump version.

## 0.1.0-alpha.1

### Patch Changes

- On publish, the README of the `latest` version is now hoisted to the packument's top-level `readme` (and `readmeFilename`) field, matching npm and verdaccio. Publish clients only send the readme inside the version manifest, so without this a package published to pnpr exposed no top-level readme for full-packument consumers and registry UIs to render.

## 0.1.0-alpha.0

### Minor Changes

- `@pnpm/pnpr` is now versioned with changesets and released from the unified release workflow. Versions switch from the `0.0.0-<datestamp>` scheme to semver, starting the 0.1.0 prerelease line at 0.1.0-alpha.0.
- The install script exits without error in the pnpm monorepo checkout, where the per-platform binary packages are not generated.
