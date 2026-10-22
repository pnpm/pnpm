---
id: config-includes
title: Splitting Settings Across Files
---

Added in: v12.15.0

`pnpm-workspace.yaml` can delegate settings to other files and to [config dependencies](./config-dependencies.md). Each delegated setting has exactly one owner, and the root manifest says which source owns it:

```yaml title="pnpm-workspace.yaml"
configDependencies:
  '@acme/pnpm-policy': 3.1.0
pnpmfile: []
include:
  ./config/security.yaml: '*'
  ./config/catalog.yaml: [catalog]
  ./config/workspace.yaml: [packages]
  '@acme/pnpm-policy': [minimumReleaseAge, trustPolicy]
```

This lets a team protect `pnpm-workspace.yaml` and the files that hold security settings, while routine changes such as catalog updates go to files anyone may edit. Tools that restrict writes by path, such as `CODEOWNERS`, a read-only mount, or an agent sandbox, can then enforce who changes what.

## Sources

`include` is a mapping from a source to the settings it may define. Only the root `pnpm-workspace.yaml` may define `include`.

* **A local file** starts with `./` or `../` and is resolved relative to the workspace root. It must be a `.yaml` or `.yml` file. Globs, directories, URLs, and absolute paths are not accepted. Every included file must exist, even if it is empty. Files that `include` does not name are never read.
* **A config dependency** is named exactly as it is declared in `configDependencies`. It always needs a list of settings. See [Settings from config dependencies](#settings-from-config-dependencies).

An included file cannot define `include`.

A source that breaks these rules fails with `ERR_PNPM_INVALID_WORKSPACE_INCLUDE`. This covers a bad or missing path, `'*'` delegated to a config dependency, a config dependency missing from `configDependencies`, and an `include` inside an included file.

### Relative paths

A relative path in an included file is relative to that file, the way a relative path in `pnpm-workspace.yaml` is relative to the workspace root. This covers settings that hold paths, such as `patchedDependencies`, `patchesDir`, `pnpmfile`, `packages`, `storeDir`, and a `scriptShell` that contains a slash. It also covers `file:`, `link:`, and `./` paths in catalogs and `overrides`, and `workspace:` paths in `overrides`. Absolute paths, and paths that start with `~` or an environment variable, are kept as written.

A patch list can therefore sit next to its patches:

```yaml title="pnpm-workspace.yaml"
include:
  ./patches/patches.yaml: [patchedDependencies]
```

```yaml title="patches/patches.yaml"
patchedDependencies:
  express@4.21.2: express@4.21.2.patch
```

`pnpm patch-commit` writes the patch to `patchesDir` and adds its entry to `patches/patches.yaml` relative to that file. pnpm also writes catalog entries and `overrides` that name a local path relative to the file that owns them.

A path is relative to the include path as written in `pnpm-workspace.yaml`, even when that path is a symlink.

## Delegation and ownership

A source is delegated either a list of settings or `'*'`.

A list both permits and reserves its settings. The file may define only the listed settings, and no other source may define them, even when the file omits them:

```yaml title="pnpm-workspace.yaml"
include:
  ./config/catalog.yaml: [catalog]
  ./config/deploy.yaml: [nodeLinker, injectWorkspacePackages]
```

Here `catalog.yaml` may define only the default catalog. Defining `minimumReleaseAge` there fails with `ERR_PNPM_SETTING_NOT_DELEGATED`. `deploy.yaml` owns `nodeLinker` even when it is empty, so `pnpm-workspace.yaml` cannot define `nodeLinker` either.

A local file delegated `'*'` may define any setting no other source owns, and owns the settings it defines. Use `'*'` only for a file that is as protected as `pnpm-workspace.yaml`.

Settings defined in `pnpm-workspace.yaml` itself belong to it. When two sources claim the same setting, pnpm fails with `ERR_PNPM_DUPLICATE_SETTING`. The order of `include` never decides between them. Different spellings of one setting, such as `virtualStoreType` and `enableGlobalVirtualStore`, share one owner.

A delegated setting that its owner omits is not set by the project. The global `config.yaml`, environment variables, and the built-in default still apply.

### Catalogs

Each catalog has one owner. Its entries are never split across sources.

* `catalog` (or `catalogs.default`) delegates the default catalog.
* `catalogs.<name>` delegates one named catalog.
* `catalogs` delegates every catalog, including catalogs added later.

```yaml title="pnpm-workspace.yaml"
include:
  ./config/catalog.yaml: [catalog]
  ./config/legacy.yaml: [catalogs.react18]
```

## Writes

Commands that change settings, such as `pnpm add` with a catalog, `pnpm approve-builds`, `pnpm patch-commit`, `pnpm audit --fix`, and `pnpm config set --location project`, write each setting to the source that owns it:

* A delegated setting goes to its file, even when the file omits it.
* A setting a `'*'` file defines stays in that file.
* A setting no source owns is added to `pnpm-workspace.yaml`.
* A setting a config dependency owns cannot be changed. pnpm fails with `ERR_PNPM_SETTING_OWNED_BY_CONFIG_DEPENDENCY`.

pnpm never edits `include`, moves a setting between files, or deletes an included file.

## Settings from config dependencies

A config dependency declares one YAML file with its settings in the `pnpm.config` field of its `package.json`:

```json title="@acme/pnpm-policy/package.json"
{
  "name": "@acme/pnpm-policy",
  "version": "3.1.0",
  "pnpm": {
    "config": "./config.yaml"
  }
}
```

```yaml title="@acme/pnpm-policy/config.yaml"
minimumReleaseAge: 1440
trustPolicy: no-downgrade
```

The file must stay inside the package. A missing or unreadable `pnpm.config` fails with `ERR_PNPM_INVALID_CONFIG_DEPENDENCY_ENTRY`. The package may define only the settings `include` delegates to it, so a new version cannot take over another setting. Some settings cannot be delegated to a config dependency. These include `configDependencies`, `pnpmfile`, `globalPnpmfile`, `bootConfig`, `packages`, `scriptShell`, and settings that hold paths, such as `patchedDependencies`, `storeDir`, and `lockfileDir`. No source can be delegated `include`.

Commands that install config dependencies fail when the settings of an included config dependency cannot be loaded. `pnpm config get` and `pnpm config list` still work before the first install, and warn that settings owned by a config dependency that is not installed are missing.

Settings from a config dependency are declarative. Loading them runs no code. A config dependency whose name follows the [plugin naming](./config-dependencies.md) pattern still loads its pnpmfile.

### `bootConfig`

Config dependencies and the pnpm version from `packageManager` are resolved before the settings of config dependencies are loaded. A config dependency's policy therefore never governs installing a config dependency. `bootConfig` sets the policy for that phase:

```yaml title="pnpm-workspace.yaml"
configDependencies:
  '@acme/pnpm-policy': 3.1.1
bootConfig:
  minimumReleaseAge: 0
include:
  '@acme/pnpm-policy': [minimumReleaseAge, trustPolicy]
```

`bootConfig` accepts `minimumReleaseAge`, `minimumReleaseAgeExclude`, `minimumReleaseAgeStrict`, `minimumReleaseAgeIgnoreMissingTime`, `trustPolicy`, `trustPolicyExclude`, and `trustPolicyIgnoreAfter`. For each setting it leaves unset or sets to `null`, the boot phase uses the regular value of that setting. For a setting a config dependency owns, that is the value from the global `config.yaml`, the environment, or the built-in default.

## Protecting the configuration

pnpm checks ownership, but it cannot tell who edits a file. Protect these paths with `CODEOWNERS`, file permissions, or a sandbox:

* `pnpm-workspace.yaml`, which decides what is loaded and who owns each setting.
* Every file delegated `'*'`.
* The pnpmfiles that are loaded. Set `pnpmfile: []` in `pnpm-workspace.yaml` when the workspace has no pnpmfile, so a new `.pnpmfile.mjs` is not picked up automatically.

```text title=".github/CODEOWNERS"
/pnpm-workspace.yaml      @acme/infra
/config/security.yaml     @acme/security
/config/catalog.yaml      @acme/developers
```

An `updateConfig` hook runs after the settings are merged and can still change any of them.
