import { PnpmError } from '@pnpm/error'
import { parseJsrSpecifier } from '@pnpm/resolving.jsr-specifier-parser'
import { parseNpmTarballUrl } from 'parse-npm-tarball-url'
import semver from 'semver'
import validateNpmPackageName from 'validate-npm-package-name'
import getVersionSelectorType from 'version-selector-type'

export interface RegistryPackageSpec {
  type: 'tag' | 'version' | 'range'
  name: string
  fetchSpec: string
  revision?: number
  normalizedBareSpecifier?: string
}

interface VersionSelector {
  type: RegistryPackageSpec['type']
  normalized: string
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
    // `npm:<version_selector>` — fall back to the outer dependency alias as
    // the package name, mirroring the named-registry shape (e.g. `gh:^1.0.0`).
    // Restricted to semver ranges/versions so unscoped package names like
    // `npm:is-positive` keep their npm package-aliasing meaning.
    if (alias && semver.validRange(bareSpecifier) != null) {
      name = alias
    } else {
      const index = bareSpecifier.lastIndexOf('@')
      if (index < 1) {
        name = bareSpecifier
        bareSpecifier = defaultTag
      } else {
        name = bareSpecifier.slice(0, index)
        bareSpecifier = bareSpecifier.slice(index + 1)
      }
    }
  }
  if (name) {
    const selector = getVersionSelectorType(bareSpecifier)
    if (selector != null) {
      return {
        ...parseRevisionSelector(selector, bareSpecifier),
        name,
      }
    }
  }
  if (bareSpecifier.startsWith(registry)) {
    const pkg = parseNpmTarballUrl(bareSpecifier)
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
    ...parseRevisionSelector(selector, spec.versionSelector ?? defaultTag),
    name: spec.npmPkgName,
    jsrPkgName: spec.jsrPkgName,
  }
}

export { BUILTIN_REGISTRIES_BY_PREFIX } from '@pnpm/constants'

export interface NamedRegistryPackageSpec extends RegistryPackageSpec {
  registryName: string
}

// Parses a named-registry specifier of the shape `<alias>:<body>` into a
// RegistryPackageSpec. Returns `null` when the specifier does not use one of
// the configured aliases, so the caller can fall through to other resolvers.
// Throws INVALID_NAMED_REGISTRY_PACKAGE_NAME when the alias matches but the
// package name is malformed (missing or empty scope/name segments, path
// separators inside the name).
// Supported shapes:
// - `<alias>:[@<owner>/]<name>[@<version_selector>]`
// - `<alias>:<version_selector>` paired with a package alias
export function parseNamedRegistrySpecifierToRegistryPackageSpec (
  rawSpecifier: string,
  knownRegistryNames: ReadonlySet<string>,
  packageAlias: string | undefined,
  defaultTag: string
): NamedRegistryPackageSpec | null {
  const colon = rawSpecifier.indexOf(':')
  if (colon <= 0) return null
  const registryName = rawSpecifier.substring(0, colon)
  if (!knownRegistryNames.has(registryName)) return null

  const body = rawSpecifier.substring(colon + 1)
  let pkgName: string
  let versionSelector: string | undefined

  if (semver.validRange(body) != null) {
    // `<alias>:<version_selector>` — fall back to the dependency alias as
    // the package name. Unresolvable without one.
    if (!packageAlias) return null
    pkgName = packageAlias
    versionSelector = body
  } else if (body[0] === '@') {
    // `<alias>:@<owner>/<name>[@<version_selector>]` — scoped package.
    const index = body.lastIndexOf('@')
    if (index === 0) {
      pkgName = body
    } else {
      pkgName = body.substring(0, index)
      versionSelector = body.substring(index + '@'.length)
    }
  } else if (packageAlias?.startsWith('@')) {
    // `<alias>:<tag>` paired with a scoped alias — body is a version
    // selector (tag/dist-tag). Mirrors GitHub Packages, where the package
    // is always scoped and a bare body is a tag.
    pkgName = packageAlias
    versionSelector = body
  } else {
    // `<alias>:<name>[@<version_selector>]` — unscoped package in body.
    const index = body.lastIndexOf('@')
    if (index < 1) {
      pkgName = body
    } else {
      pkgName = body.substring(0, index)
      versionSelector = body.substring(index + '@'.length)
    }
    if (!pkgName) return null
  }

  // The name is used in registry URLs and metadata cache file paths, so
  // anything that is not a valid npm package name must never make it through.
  if (!validateNpmPackageName(pkgName).validForOldPackages) {
    throw new PnpmError(
      'INVALID_NAMED_REGISTRY_PACKAGE_NAME',
      `The package name '${pkgName}' in named registry '${registryName}:' is invalid`
    )
  }

  const selector = getVersionSelectorType(versionSelector ?? defaultTag)
  if (selector == null) return null

  return {
    ...parseRevisionSelector(selector, versionSelector ?? defaultTag),
    name: pkgName,
    registryName,
  }
}

function parseRevisionSelector (
  selector: VersionSelector,
  rawSelector: string
): Pick<RegistryPackageSpec, 'fetchSpec' | 'revision' | 'type'> {
  if (selector.type !== 'version') {
    return { fetchSpec: selector.normalized, type: selector.type }
  }
  const normalizedInput = rawSelector.trim()
  const buildIndex = normalizedInput.indexOf('+')
  if (buildIndex === -1) {
    return { fetchSpec: selector.normalized, type: selector.type }
  }
  const build = normalizedInput.slice(buildIndex + 1)
  if (build.length < 2 || build[0] !== 'r' || build.includes('.')) {
    return { fetchSpec: selector.normalized, type: selector.type }
  }
  const digits = build.slice(1)
  if (![...digits].every((char) => char >= '0' && char <= '9')) {
    return { fetchSpec: selector.normalized, type: selector.type }
  }
  const revision = Number(digits)
  if ((digits.length > 1 && digits[0] === '0') || !Number.isSafeInteger(revision)) {
    throw new PnpmError('INVALID_REVISION_SPEC', `Invalid registry revision in version specifier "${rawSelector}"`)
  }
  return {
    fetchSpec: selector.normalized,
    revision,
    type: selector.type,
  }
}
