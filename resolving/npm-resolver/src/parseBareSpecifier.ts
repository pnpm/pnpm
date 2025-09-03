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
    fetchSpec: selector.normalized,
    name: spec.npmPkgName,
    type: selector.type,
    jsrPkgName: spec.jsrPkgName,
  }
}
