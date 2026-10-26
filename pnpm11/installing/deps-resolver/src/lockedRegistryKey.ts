import { isSameRegistry, pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import * as dp from '@pnpm/deps.path'
import {
  keyPrefixForAlias,
  keyPrefixForRegistry,
  registryOfPackage,
  registryOfPrefix,
  rekeyDepPath,
} from '@pnpm/lockfile.utils'
import { parseJsrSpecifier } from '@pnpm/resolving.jsr-specifier-parser'
import type { RegistryContext } from '@pnpm/types'
import getVerSelType from 'version-selector-type'

import { unwrapPackageName } from './unwrapPackageName.js'

/**
 * A wanted specifier the lockfile reuse gate can judge: the package it names,
 * the registry it resolves against, the selector checked against a locked
 * version, and the prefix the key of a package it resolves carries.
 */
interface WantedRegistrySpec {
  name: string
  registry: string
  range: string
  keyPrefix: string | undefined
}

interface WantedDep {
  alias: string
  bareSpecifier: string
}

/**
 * The part of the wanted specifier that the locked package's version is
 * checked against, or `undefined` when the locked package cannot satisfy the
 * wanted dependency whatever its version.
 *
 * A registry specifier is satisfied only by a package of the name it names,
 * locked from the registry it resolves against: a plain range resolves
 * against the registry the scope of the package routes to, a `<prefix>:`
 * specifier against the prefix's registry, and a key names its registry the
 * same way. So an entry from another registry never satisfies, however either
 * is spelled, while a key another prefix of the same registry qualifies does.
 * Whether the key is the one a fresh resolve mints is
 * {@link lockedKeyIsCurrent}'s question; the version pin holds either way.
 */
export function getRangeOfLockedPackage (
  wantedDep: WantedDep,
  locked: { name: string, registryName?: string },
  registries: RegistryContext
): string | undefined {
  const wanted = parseWantedRegistrySpec(wantedDep, registries)
  if (wanted == null) {
    // A git or tarball specifier may resolve to a package of any name.
    return locked.registryName == null ? wantedDep.bareSpecifier : undefined
  }
  if (wanted.name !== locked.name) return undefined
  const lockedRegistry = registryOfPackage(registries, locked)
  return lockedRegistry != null && isSameRegistry(lockedRegistry, wanted.registry) ? wanted.range : undefined
}

/**
 * Whether `depPath` is the key a fresh resolve of the wanted dependency mints,
 * so that a reused subtree records the key the registry configuration writes
 * today. A plain range mints the prefix its registry declares. A `<prefix>:`
 * specifier mints the prefix the keys of that registry carry: a declared
 * prefix, or a built-in alias as spelled. A key that is not a registry
 * version is left as it is.
 */
export function lockedKeyIsCurrent (wantedDep: WantedDep, depPath: string, registries: RegistryContext): boolean {
  const { version, registryName } = dp.parse(depPath)
  if (version == null) return true
  const wanted = parseWantedRegistrySpec(wantedDep, registries)
  if (wanted == null) return rekeyDepPath(registries, depPath) == null
  return wanted.keyPrefix === registryName
}

/** Whether the specifier is `<prefix>:` of a declared prefix or a built-in alias. */
export function isNamedRegistrySpecifier (wantedDep: WantedDep, registries: RegistryContext): boolean {
  return parsePrefixedSpec(wantedDep, registries) != null
}

/**
 * `undefined` for a specifier that is neither a version selector (a range, a
 * version, or a tag), an `npm:` or `jsr:` alias, nor a `<prefix>:` specifier
 * of a declared prefix or a built-in alias.
 */
function parseWantedRegistrySpec ({ alias, bareSpecifier }: WantedDep, registries: RegistryContext): WantedRegistrySpec | undefined {
  if (bareSpecifier.startsWith('npm:')) {
    const target = unwrapPackageName(alias, bareSpecifier)
    return routedSpec(registries, target.pkgName, target.bareSpecifier)
  }
  if (bareSpecifier.startsWith('jsr:')) {
    return parseJsrSpec({ alias, bareSpecifier }, registries)
  }
  const prefixed = parsePrefixedSpec({ alias, bareSpecifier }, registries)
  if (prefixed != null) return prefixed
  return getVerSelType(bareSpecifier) == null ? undefined : routedSpec(registries, alias, bareSpecifier)
}

function routedSpec (registries: RegistryContext, name: string, range: string): WantedRegistrySpec {
  const registry = pickRegistryForPackage(registries.registriesByScope, name)
  return { name, registry, range, keyPrefix: keyPrefixForRegistry(registries, registry) }
}

function parseJsrSpec (wantedDep: WantedDep, registries: RegistryContext): WantedRegistrySpec | undefined {
  let jsrSpec: ReturnType<typeof parseJsrSpecifier>
  try {
    jsrSpec = parseJsrSpecifier(wantedDep.bareSpecifier, wantedDep.alias)
  } catch {
    return undefined
  }
  if (jsrSpec == null) return undefined
  return routedSpec(registries, jsrSpec.npmPkgName, jsrSpec.versionSelector ?? 'latest')
}

/** `<prefix>:[<name>@]<range>`; a body that names no package names the alias. */
function parsePrefixedSpec ({ alias, bareSpecifier }: WantedDep, registries: RegistryContext): WantedRegistrySpec | undefined {
  const colon = bareSpecifier.indexOf(':')
  if (colon <= 0) return undefined
  const prefix = bareSpecifier.slice(0, colon)
  const registry = registryOfPrefix(registries, prefix)
  if (registry == null) return undefined
  const body = bareSpecifier.slice(colon + 1)
  // `@scope/name@range` splits at the last `@`. A bare range has none, or only
  // the leading one of a scope, which never delimits a version.
  const versionDelimiter = body.lastIndexOf('@')
  const [name, range] = versionDelimiter > 0
    ? [body.slice(0, versionDelimiter), body.slice(versionDelimiter + 1)]
    : [alias, body]
  return { name, registry, range, keyPrefix: keyPrefixForAlias(registries, prefix) }
}
