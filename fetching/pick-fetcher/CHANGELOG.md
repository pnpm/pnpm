# @pnpm/pick-fetcher

## 1001.0.0

### Major Changes

- d1edf73: Rename Resolution to AtomicResolution. Add support for binary resolution.

## 1000.1.0

### Minor Changes

- 1a07b8f: Added support for resolving and downloading the Node.js runtime specified in the [devEngines](https://github.com/openjs-foundation/package-metadata-interoperability-collab-space/issues/15) field of `package.json`.

  Usage example:

  ```json
  {
    "devEngines": {
      "runtime": {
        "name": "node",
        "version": "^24.4.0",
        "onFail": "download"
      }
    }
  }
  ```

  When running `pnpm install`, pnpm will resolve Node.js to the latest version that satisfies the specified range and install it as a dependency of the project. As a result, when running scripts, the locally installed Node.js version will be used.

  Unlike the existing options, `useNodeVersion` and `executionEnv.nodeVersion`, this new field supports version ranges, which are locked to exact versions during installation. The resolved version is stored in the pnpm lockfile, along with an integrity checksum for future validation of the Node.js content's validity.

  Related PR: [#9755](https://github.com/pnpm/pnpm/pull/9755).

## 1000.0.1

### Patch Changes

- 6acf819: Remove the blanket variant from the `Resolution` type, making it stricter and more useful.

## 3.0.0

### Major Changes

- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.

## 2.0.1

### Patch Changes

- f394cfccd: Don't update git-hosted dependencies when adding an unrelated dependency [#7008](https://github.com/pnpm/pnpm/issues/7008).

## 2.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

## 1.0.0

### Major Changes

- 7a17f99ab: Refactor `tarball-fetcher` and separate it into more specific fetchers, such as `localTarball`, `remoteTarball` and `gitHostedTarball`.

### Minor Changes

- 23984abd1: Add hook for adding custom fetchers.
