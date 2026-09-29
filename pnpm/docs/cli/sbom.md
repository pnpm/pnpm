---
id: sbom
title: pnpm sbom
---

Added in: v11.0.0

Generate a Software Bill of Materials (SBOM) for the project.

Supported formats:

- **CycloneDX 1.7** (JSON)
- **SPDX 2.3** (JSON)

## Usage

```sh
pnpm sbom --sbom-format cyclonedx
pnpm sbom --sbom-format spdx
pnpm sbom --sbom-format cyclonedx --lockfile-only
pnpm sbom --sbom-format spdx --prod
pnpm sbom --sbom-format cyclonedx --out sbom.cdx.json
pnpm sbom --sbom-format cyclonedx --split
pnpm sbom --sbom-format cyclonedx --exclude-peers
```

Inside a workspace, `pnpm sbom` supports filtering. When a single workspace package is selected, the root component in the SBOM uses that package's metadata.

Since v11.20.0, a component resolved from a [named registry](../settings/dependency-resolution.md#namedregistries) carries the purl `repository_url` qualifier (for example `pkg:npm/foo@1.0.0?repository_url=https%3A%2F%2Fnpm.work.example.com%2F`), so the same name and version served by two registries does not collapse onto one component. Credentials that a `namedRegistries` URL may embed — userinfo or a query string — are stripped from the qualifier, while the origin and path are kept, since two registries can differ only by path.

CycloneDX output marks components reachable only through `devDependencies` with `scope: "excluded"` and the `cdx:npm:package:development` property. Runtime components, including installed optional dependencies, use the default required scope.

## Options

### --sbom-format &lt;cyclonedx|spdx\>

The SBOM output format. This option is required. Supported values: `cyclonedx`, `spdx`.

### --sbom-type &lt;library|application\>

* Default: **library**

The component type for the root package.

### --sbom-spec-version &lt;version\>

Added in: v11.1.0

* Default: **1.7**
* Type: **1.5**, **1.6**, **1.7**

The CycloneDX specification version to emit. Only valid with `--sbom-format cyclonedx`.

### --lockfile-only

Only use lockfile data (skip reading from the store).

### --sbom-authors &lt;names\>

Comma-separated list of SBOM authors. Written to `metadata.authors` in the CycloneDX output.

### --sbom-supplier &lt;name\>

SBOM supplier name. Written to `metadata.supplier` in the CycloneDX output.

### --out &lt;path\>

Added in: v11.8.0

Write the SBOM to a file instead of stdout.

Use `%s` in the path as a placeholder for the package name and `%v` as a placeholder for the package version. In a workspace, a path containing `%s` writes one SBOM per selected package:

```sh
pnpm sbom --sbom-format cyclonedx --out out/%s.cdx.json
pnpm sbom --sbom-format cyclonedx --out out/%s-%v.cdx.json
```

### --split

Added in: v11.8.0

Generate a separate SBOM for each selected workspace package. Without `--out`, the SBOMs are printed to stdout as NDJSON, one JSON document per line.

When `--split` is combined with `--out`, the output path must contain `%s`.

### --exclude-peers

Added in: v11.9.0

Exclude peer dependencies from the SBOM. Dependencies reachable only through those peers are also excluded.

This is useful with `auto-install-peers` because peer dependencies are resolved into the lockfile and otherwise look the same as regular dependencies.

### --prod, -P

Only include `dependencies` and `optionalDependencies`.

### --dev, -D

Only include `devDependencies`.

### --no-optional

Don't include `optionalDependencies`.

### --filter &lt;package_selector\>

[Read more about filtering.](../filtering.md)
