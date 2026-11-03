import { isSameRegistry, pickAliasKeyPrefix, pickDeclaredRegistryPrefix, pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import { BUILTIN_REGISTRIES_BY_PREFIX } from '@pnpm/constants'
import * as dp from '@pnpm/deps.path'
import type { LockfileObject } from '@pnpm/lockfile.types'
import type { DepPath, RegistriesByScope, RegistryContext } from '@pnpm/types'

/**
 * The registry a `<prefix>:` specifier or a registry-qualified key names: a
 * declared prefix or a built-in alias. `undefined` when nothing declares
 * `prefix`.
 */
export function registryOfPrefix (registries: RegistryContext, prefix: string): string | undefined {
  // Own properties only: the prefix comes out of a lockfile key, which must
  // not reach `Object.prototype`. A declared prefix wins over a built-in one.
  const { registriesByPrefix } = registries
  if (registriesByPrefix != null && Object.hasOwn(registriesByPrefix, prefix)) return registriesByPrefix[prefix]
  return Object.hasOwn(BUILTIN_REGISTRIES_BY_PREFIX, prefix) ? BUILTIN_REGISTRIES_BY_PREFIX[prefix] : undefined
}

/**
 * The registry the package keyed `depPath` resolved from: the one its prefix
 * names, or for a plain key the one its scope routes to. `undefined` when the
 * key is qualified by a prefix nothing declares.
 */
export function registryOfDepPath (registries: RegistryContext, depPath: string): string | undefined {
  const { name, registryName } = dp.parse(depPath)
  return name == null ? undefined : registryOfPackage(registries, { name, registryName })
}

/**
 * The registry of a locked package: the one its key's prefix names, or for a
 * plain key the one its scope was routed to when the lockfile was resolved
 * (`lockedRegistriesByScope`), or routes to now. See {@link registryOfDepPath}.
 */
export function registryOfPackage (registries: RegistryContext, { name, registryName }: { name: string, registryName?: string }): string | undefined {
  return registryName == null
    ? pickRegistryForPackage(lockedRegistriesByScope(registries), name)
    : registryOfPrefix(registries, registryName)
}

/**
 * The routes a plain key resolved through: the recorded scope routes over the
 * current default registry, which the record leaves out, or the current
 * routes without a record.
 */
function lockedRegistriesByScope (registries: RegistryContext): RegistriesByScope {
  if (registries.lockedRegistriesByScope == null) return registries.registriesByScope
  return { ...registries.lockedRegistriesByScope, default: registries.registriesByScope.default }
}

export interface MovedRegistryKey {
  key: DepPath
  from: string
  to: string
}

/**
 * The smallest plain `packages:` key whose package resolved from a registry
 * its scope no longer routes to, with that registry and the current one.
 * `undefined` without a record of the routes the lockfile was resolved under.
 * The recorded integrity and children of such a key belong to the old
 * registry, so a resolving install resolves the package again from the new
 * one. A frozen install installs the lockfile as recorded.
 */
export function findMovedRegistryKey (lockfile: LockfileObject, registries: RegistryContext): MovedRegistryKey | undefined {
  if (registries.lockedRegistriesByScope == null) return undefined
  const locked = lockedRegistriesByScope(registries)
  let moved: MovedRegistryKey | undefined
  for (const key of Object.keys(lockfile.packages ?? {}) as DepPath[]) {
    if (moved != null && moved.key < key) continue
    const { name, version, registryName } = dp.parse(key)
    if (name == null || version == null || registryName != null) continue
    const from = pickRegistryForPackage(locked, name)
    const to = pickRegistryForPackage(registries.registriesByScope, name)
    if (!isSameRegistry(from, to)) {
      moved = { key, from, to }
    }
  }
  return moved
}

/**
 * The prefix the key of a package resolved from `registry` carries. See
 * `pickDeclaredRegistryPrefix`.
 */
export function keyPrefixForRegistry (registries: RegistryContext, registry: string): string | undefined {
  return pickDeclaredRegistryPrefix(registries, registry)
}

/**
 * The prefix the key of `name`, resolved by a `<prefix>:` specifier, carries.
 * See `pickAliasKeyPrefix`. `undefined` for a plain key, and when nothing
 * declares `prefix`.
 */
export function keyPrefixForAlias (registries: RegistryContext, prefix: string, name: string): string | undefined {
  const registry = registryOfPrefix(registries, prefix)
  if (registry == null) return undefined
  return pickAliasKeyPrefix(registries, { alias: prefix, name, registry })
}

/**
 * The key this configuration writes for the package of `depPath` in place of
 * `depPath`, when it differs: a plain key of a package whose registry declares
 * a prefix, a key qualified by a prefix other than the one the keys of its
 * registry carry, or a qualified key of a package the default registry serves
 * under a plain key. A key that is not a registry version, or that is
 * qualified by a prefix nothing declares, is left alone.
 */
export function rekeyDepPath (registries: RegistryContext, depPath: string): DepPath | undefined {
  const { name, version, registryName } = dp.parse(depPath)
  if (name == null || version == null) return undefined
  const currentPrefix = currentKeyPrefix(registries, name, registryName)
  if (currentPrefix === registryName || currentPrefix === UNDECLARED) return undefined
  const peerSuffix = depPath.slice(dp.removeSuffix(depPath).length)
  return (currentPrefix == null ? `${name}@${version}${peerSuffix}` : `${name}@${currentPrefix}:${version}${peerSuffix}`) as DepPath
}

const UNDECLARED = Symbol('undeclared prefix')

function currentKeyPrefix (registries: RegistryContext, name: string, registryName: string | undefined): string | undefined | typeof UNDECLARED {
  if (registryName == null) {
    return keyPrefixForRegistry(registries, pickRegistryForPackage(registries.registriesByScope, name))
  }
  if (registryOfPrefix(registries, registryName) == null) return UNDECLARED
  return keyPrefixForAlias(registries, registryName, name)
}

export interface OutdatedRegistryKey {
  key: DepPath
  current: DepPath
}

/**
 * The smallest `packages:` key that is not the one the registry configuration
 * writes for its package (see {@link rekeyDepPath}), with that current key.
 * A resolving install re-keys such a lockfile, which merges the entries that
 * an older pnpm, or an older registry configuration, keyed apart. A frozen
 * install installs the keys as recorded.
 */
export function findOutdatedRegistryKey (lockfile: LockfileObject, registries: RegistryContext): OutdatedRegistryKey | undefined {
  // Without a declared prefix only a qualified key can be outdated, and every
  // qualified key has a colon.
  const plainKeysAreCurrent = registries.registriesByPrefix == null || Object.keys(registries.registriesByPrefix).length === 0
  let outdated: OutdatedRegistryKey | undefined
  for (const key of Object.keys(lockfile.packages ?? {}) as DepPath[]) {
    if (outdated != null && outdated.key < key) continue
    if (plainKeysAreCurrent && !key.includes(':')) continue
    const current = rekeyDepPath(registries, key)
    if (current != null) {
      outdated = { key, current }
    }
  }
  return outdated
}
