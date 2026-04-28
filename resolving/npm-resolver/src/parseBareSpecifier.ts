import { PnpmError } from '@pnpm/error'
import { parseJsrSpecifier } from '@pnpm/resolving.jsr-specifier-parser'
import parseNpmTarballUrl from 'parse-npm-tarball-url'
import getVersionSelectorType from 'version-selector-type'

export interface RegistryPackageSpec {
  type: 'tag' | 'version' | 'range'
  name: string
  fetchSpec: string
  normalizedBareSpecifier?: string
}

export function parseBareSpecifier (
  bareSpecifier: string,
  alias: string | undefined,
  defaultTag: string,
  registry: string
): RegistryPackageSpec | null {
  let name = alias
  if (bareSpecifier.startsWith('npm:')) {
    bareSpecifier = bareSpecifier.slice(4)
    const index = bareSpecifier.lastIndexOf('@')
    if (index < 1) {
      name = bareSpecifier
      bareSpecifier = defaultTag
    } else {
      name = bareSpecifier.slice(0, index)
      bareSpecifier = bareSpecifier.slice(index + 1)
    }
  }
  if (name) {
    const selector = getVersionSelectorType(bareSpecifier)
    if (selector != null) {
      return {
        fetchSpec: selector.normalized,
        name,
        type: selector.type,
      }
    }
  }
  if (bareSpecifier.startsWith(registry)) {
    const pkg = parseNpmTarballUrl.default(bareSpecifier)
    if (pkg != null) {
      return {
        fetchSpec: pkg.version,
        name: pkg.name,
        normalizedBareSpecifier: bareSpecifier,
        type: 'version',
      }
    }
  }
  return null
}

export interface JsrRegistryPackageSpec extends RegistryPackageSpec {
  jsrPkgName: string
}

export function parseJsrSpecifierToRegistryPackageSpec (
  rawSpecifier: string,
  alias: string | undefined,
  defaultTag: string
): JsrRegistryPackageSpec | null {
  const spec = parseJsrSpecifier(rawSpecifier, alias)
  if (!spec?.npmPkgName) return null

  const selector = getVersionSelectorType(spec.versionSelector ?? defaultTag)
  if (selector == null) return null

  return {
    fetchSpec: selector.normalized,
    name: spec.npmPkgName,
    type: selector.type,
    jsrPkgName: spec.jsrPkgName,
  }
}

export const BUILTIN_NAMED_REGISTRIES: Readonly<Record<string, string>> = Object.freeze({
  gh: 'https://npm.pkg.github.com/',
})

export interface NamedRegistryPackageSpec extends RegistryPackageSpec {
  registryAlias: string
}

// Parses a named-registry specifier of the shape `<alias>:<body>` into a
// RegistryPackageSpec. Returns `null` when the specifier does not use one of
// the configured aliases, so the caller can fall through to other resolvers.
// Supported shapes (mirrors the `gh:` prefix, which is the canonical example):
// - `<alias>:@<owner>/<name>[@<version_selector>]`
// - `<alias>:<version_selector>` when paired with a scoped package alias
export function parseNamedRegistrySpecifierToRegistryPackageSpec (
  rawSpecifier: string,
  knownAliases: ReadonlySet<string>,
  packageAlias: string | undefined,
  defaultTag: string
): NamedRegistryPackageSpec | null {
  const colon = rawSpecifier.indexOf(':')
  if (colon <= 0) return null
  const registryAlias = rawSpecifier.substring(0, colon)
  if (!knownAliases.has(registryAlias)) return null

  const body = rawSpecifier.substring(colon + 1)
  let pkgName: string
  let versionSelector: string | undefined

  if (body[0] === '@') {
    // syntax: <alias>:@<owner>/<name>[@<version_selector>]
    const index = body.lastIndexOf('@')
    if (index === 0) {
      pkgName = body
    } else {
      pkgName = body.substring(0, index)
      versionSelector = body.substring(index + '@'.length)
    }
  } else if (packageAlias?.startsWith('@')) {
    // syntax: <alias>:<version_selector> paired with a scoped package alias
    pkgName = packageAlias
    versionSelector = body
  } else {
    // No scoped alias means we cannot know the package name — let other
    // resolvers try.
    return null
  }

  validateScopedPackageName(pkgName, registryAlias)

  const selector = getVersionSelectorType(versionSelector ?? defaultTag)
  if (selector == null) return null

  return {
    fetchSpec: selector.normalized,
    name: pkgName,
    type: selector.type,
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
