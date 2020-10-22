# @pnpm/types

## 6.3.1

### Patch Changes

- b5d694e7f: Use pnpm.overrides instead of resolutions. Still support resolutions for partial compatibility with Yarn and for avoiding a breaking change.

## 6.3.0

### Minor Changes

- d54043ee4: A new optional field added to the ProjectManifest type: resolutions.

## 6.2.0

### Minor Changes

- db17f6f7b: Add Project and ProjectsGraph types.

## 6.1.0

### Minor Changes

- 71a8c8ce3: Added a new type: HoistedDependencies.

## 6.0.0

### Major Changes

- da091c711: Remove state from store. The store should not store the information about what projects on the computer use what dependencies. This information was needed for pruning in pnpm v4. Also, without this information, we cannot have the `pnpm store usages` command. So `pnpm store usages` is deprecated.

## 6.0.0-alpha.0

### Major Changes

- da091c71: Remove state from store. The store should not store the information about what projects on the computer use what dependencies. This information was needed for pruning in pnpm v4. Also, without this information, we cannot have the `pnpm store usages` command. So `pnpm store usages` is deprecated.
