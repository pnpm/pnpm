import { PnpmError } from '@pnpm/error'

export interface NamedRegistrySpec {
  pkgName: string
  // A versionSelector may be a semver range (e.g. ^1.0.0), exact version (e.g. 2.3.4), or a dist-tag (e.g. "latest").
  versionSelector?: string
  registryAlias: string
}

export const DEFAULT_GH_REGISTRY = 'https://npm.pkg.github.com/'

export const BUILTIN_GH_ALIAS = 'gh'

// Built-in named-registry aliases. Users can add more via `namedRegistries` in
// `pnpm-workspace.yaml`. The `gh` alias mirrors vlt's convention and lets users
// install from GitHub Packages without configuring a scope-wide registry.
export const BUILTIN_NAMED_REGISTRIES: Readonly<Record<string, string>> = Object.freeze({
  [BUILTIN_GH_ALIAS]: DEFAULT_GH_REGISTRY,
})

// Parses a named-registry specifier of the shape `<alias>:<body>`. Shared between
// the built-in `gh:` prefix and any user-defined aliases configured in
// `pnpm-workspace.yaml`. Returns `null` when the specifier does not use one of
// the known aliases, so the caller can fall through to other resolvers.
// Supported shapes (mirrors the `gh:` prefix, which is the canonical example):
// - `<alias>:@<owner>/<name>[@<version_selector>]`
// - `<alias>:<version_selector>` when paired with a scoped package alias
export function parseNamedRegistrySpecifier (
  rawSpecifier: string,
  knownAliases: ReadonlySet<string>,
  packageAlias?: string
): NamedRegistrySpec | null {
  const colon = rawSpecifier.indexOf(':')
  if (colon <= 0) return null
  const registryAlias = rawSpecifier.substring(0, colon)
  if (!knownAliases.has(registryAlias)) return null

  const body = rawSpecifier.substring(colon + 1)

  // syntax: <alias>:@<owner>/<name>[@<version_selector>]
  if (body[0] === '@') {
    const index = body.lastIndexOf('@')

    // syntax: <alias>:@<owner>/<name>
    if (index === 0) {
      validateScopedPackageName(body, registryAlias)
      return {
        pkgName: body,
        registryAlias,
      }
    }

    // syntax: <alias>:@<owner>/<name>@<version_selector>
    const pkgName = body.substring(0, index)
    validateScopedPackageName(pkgName, registryAlias)
    return {
      pkgName,
      versionSelector: body.substring(index + '@'.length),
      registryAlias,
    }
  }

  // Otherwise expect a bare version selector paired with a scoped alias.
  if (!packageAlias || !packageAlias.startsWith('@')) {
    return null
  }

  validateScopedPackageName(packageAlias, registryAlias)
  return {
    versionSelector: body,
    pkgName: packageAlias,
    registryAlias,
  }
}

function validateScopedPackageName (pkgName: string, registryAlias: string): void {
  if (pkgName[0] !== '@') {
    throw new PnpmError(
      'MISSING_NAMED_REGISTRY_PACKAGE_SCOPE',
      `Package '${pkgName}' from named registry '${registryAlias}:' must have a scope (e.g. '@owner/name')`
    )
  }
  const sepIndex = pkgName.indexOf('/')
  if (sepIndex === -1 || sepIndex === pkgName.length - 1) {
    throw new PnpmError(
      'INVALID_NAMED_REGISTRY_PACKAGE_NAME',
      `The package name '${pkgName}' in named registry '${registryAlias}:' is invalid`
    )
  }
}
