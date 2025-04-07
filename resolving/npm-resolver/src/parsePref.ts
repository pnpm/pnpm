import { PnpmError } from '@pnpm/error'
import parseNpmTarballUrl from 'parse-npm-tarball-url'
import getVersionSelectorType from 'version-selector-type'

export interface RegistryPackageSpec {
  type: 'tag' | 'version' | 'range'
  name: string
  fetchSpec: string
  normalizedPref?: string
}

function parseNameAndTag (pref: string, defaultTag: string): [string, string] {
  const index = pref.lastIndexOf('@')
  if (index < 1) {
    return [pref, defaultTag]
  }
  const name = pref.slice(0, index)
  const tag = pref.slice(index + '@'.length)
  return [name, tag]
}

function parsePrefFromNameAndTag (
  name: string | undefined,
  tag: string,
  registry: string
): RegistryPackageSpec | null {
  if (name) {
    const selector = getVersionSelectorType(tag)
    if (selector != null) {
      return {
        fetchSpec: selector.normalized,
        name,
        type: selector.type,
      }
    }
  }
  if (tag.startsWith(registry)) {
    const pkg = parseNpmTarballUrl(tag)
    if (pkg != null) {
      return {
        fetchSpec: pkg.version,
        name: pkg.name,
        normalizedPref: tag,
        type: 'version',
      }
    }
  }
  return null
}

export function parsePref (
  pref: string,
  alias: string | undefined,
  defaultTag: string,
  registry: string
): RegistryPackageSpec | null {
  let name = alias
  if (pref.startsWith('npm:')) {
    [name, pref] = parseNameAndTag(pref.slice('npm:'.length), defaultTag)
  }
  return parsePrefFromNameAndTag(name, pref, registry)
}

export function parseJsrPref (
  pref: string,
  alias: string | undefined,
  defaultTag: string,
  registry: string
): RegistryPackageSpec | null {
  if (!pref.startsWith('jsr:')) return null
  pref = pref.slice('jsr:'.length)
  let spec = parsePref(pref, alias, defaultTag, registry)
  if (!spec) {
    const [name, tag] = parseNameAndTag(pref, defaultTag)
    spec = parsePrefFromNameAndTag(name, tag, registry)
  }
  if (!spec) {
    throw new PnpmError('INVALID_JSR_SPECIFICATION', `Cannot parse '${pref}' as an npm specification`)
  }
  if (!spec.name.startsWith('@')) {
    throw new PnpmError('MISSING_JSR_PACKAGE_SCOPE', 'Package names from JSR must have scopes')
  }
  const jsrNameSuffix = spec.name.replace('@', '').replace('/', '__') // not replaceAll because we only replace the first of each character
  spec.name = `@jsr/${jsrNameSuffix}`
  return spec
}
