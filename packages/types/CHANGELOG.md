# @pnpm/types

## 8.2.0

### Minor Changes

- fb5bbfd7a: A new setting added: `pnpm.peerDependencyRules.allowAny`. `allowAny` is an array of package name patterns, any peer dependency matching the pattern will be resolved from any version, regardless of the range specified in `peerDependencies`. For instance:

  ```
  {
    "pnpm": {
      "peerDependencyRules": {
        "allowAny": ["@babel/*", "eslint"]
      }
    }
  }
  ```

  The above setting will mute any warnings about peer dependency version mismatches related to `@babel/` packages or `eslint`.

## 8.1.0

### Minor Changes

- 4d39e4a0c: Add new setting to pnpm field of the manifest: allowedDeprecatedVersions.

## 8.0.1

### Patch Changes

- 18ba5e2c0: Add typesVersions to PUBLISH_CONFIG_WHITELIST

## 8.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.

### Minor Changes

- d504dc380: New fields add to package.json type.

## 7.10.0

### Minor Changes

- b138d048c: New optional field supported: `onlyBuiltDependencies`.

## 7.9.0

### Minor Changes

- 26cd01b88: New field added to package.json.pnpm section: peerDependencyRules.

## 7.8.0

### Minor Changes

- b5734a4a7: Add `resolvedFrom` field to `BadPeerDependencyIssues`.

## 7.7.1

### Patch Changes

- 6493e0c93: add readme file to published package.json file

## 7.7.0

### Minor Changes

- ba9b2eba1: Add types for peer dependency issues.

## 7.6.0

### Minor Changes

- 302ae4f6f: Support async hooks

## 7.5.0

### Minor Changes

- 4ab87844a: New optional field added to `dependenciesMeta`: `injected`.

## 7.4.0

### Minor Changes

- b734b45ea: Add `publishConfig.executableFiles`.

## 7.3.0

### Minor Changes

- 8e76690f4: New optional field added to the manifest type (`package.json`): `pnpm.packageExtensions.

## 7.2.0

### Minor Changes

- 724c5abd8: support "publishConfig.directory" field

## 7.1.0

### Minor Changes

- 97c64bae4: An optional `dir` parameter added to the `ReadPackageHook` function. The `dir` parameter is defined when the hook runs on project manifests and defined the root of the project.

## 7.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

## 6.4.0

### Minor Changes

- 9ad8c27bf: Allow to ignore builds of specified dependencies through the `pnpm.neverBuiltDependencies` field in `package.json`.

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
