# @pnpm/tools.plugin-commands-self-updater

## 1.0.9

### Patch Changes

- Updated dependencies [6014522]
  - @pnpm/plugin-commands-installation@17.2.7
  - @pnpm/client@11.1.12
  - @pnpm/cli-utils@4.0.7
  - @pnpm/config@21.8.5
  - @pnpm/link-bins@10.0.11

## 1.0.8

### Patch Changes

- @pnpm/plugin-commands-installation@17.2.6
- @pnpm/client@11.1.11
- @pnpm/cli-utils@4.0.6
- @pnpm/config@21.8.4
- @pnpm/link-bins@10.0.11

## 1.0.7

### Patch Changes

- @pnpm/plugin-commands-installation@17.2.5

## 1.0.6

### Patch Changes

- Updated dependencies [83681da]
  - @pnpm/plugin-commands-installation@17.2.4
  - @pnpm/config@21.8.4
  - @pnpm/error@6.0.2
  - @pnpm/cli-utils@4.0.6
  - @pnpm/link-bins@10.0.11
  - @pnpm/read-project-manifest@6.0.9
  - @pnpm/client@11.1.10

## 1.0.5

### Patch Changes

- Updated dependencies [ad1fd64]
- Updated dependencies [eeb76cd]
  - @pnpm/plugin-commands-installation@17.2.3

## 1.0.4

### Patch Changes

- @pnpm/cli-meta@6.2.2
- @pnpm/cli-utils@4.0.5
- @pnpm/config@21.8.3
- @pnpm/pick-registry-for-package@6.0.7
- @pnpm/client@11.1.9
- @pnpm/link-bins@10.0.10
- @pnpm/plugin-commands-installation@17.2.2
- @pnpm/read-project-manifest@6.0.8

## 1.0.3

### Patch Changes

- @pnpm/plugin-commands-installation@17.2.1

## 1.0.2

### Patch Changes

- Updated dependencies [7ee59a1]
  - @pnpm/plugin-commands-installation@17.2.0
  - @pnpm/cli-meta@6.2.1
  - @pnpm/cli-utils@4.0.4
  - @pnpm/config@21.8.2
  - @pnpm/pick-registry-for-package@6.0.6
  - @pnpm/client@11.1.8
  - @pnpm/link-bins@10.0.9
  - @pnpm/read-project-manifest@6.0.7

## 1.0.1

### Patch Changes

- @pnpm/plugin-commands-installation@17.1.1

## 1.0.0

### Major Changes

- eb8bf2a: Added a new command for upgrading pnpm itself when it isn't managed by Corepack: `pnpm self-update`. This command will work, when pnpm was installed via the standalone script from the [pnpm installation page](https://pnpm.io/installation#using-a-standalone-script) [#8424](https://github.com/pnpm/pnpm/pull/8424).

  When executed in a project that has a `packageManager` field in its `package.json` file, pnpm will update its version in the `packageManager` field.

### Patch Changes

- Updated dependencies [eb8bf2a]
  - @pnpm/tools.path@1.0.0
  - @pnpm/plugin-commands-installation@17.1.0
  - @pnpm/cli-meta@6.2.0
  - @pnpm/cli-utils@4.0.3
