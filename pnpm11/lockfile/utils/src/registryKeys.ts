import { pickDeclaredRegistryPrefix, pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import { BUILTIN_REGISTRIES_BY_PREFIX } from '@pnpm/constants'
import * as dp from '@pnpm/deps.path'
import type { LockfileObject } from '@pnpm/lockfile.types'
import type { DepPath, RegistryContext } from '@pnpm/types'

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
 * plain key the one its scope routes to. See {@link registryOfDepPath}.
 */
export function registryOfPackage (registries: RegistryContext, { name, registryName }: { name: string, registryName?: string }): string | undefined {
  return registryName == null
    ? pickRegistryForPackage(registries.registriesByScope, name)
    : registryOfPrefix(registries, registryName)
}

/**
 * The prefix the key of a package resolved from `registry` carries. See
 * `pickDeclaredRegistryPrefix`.
 */
export function keyPrefixForRegistry (registries: RegistryContext, registry: string): string | undefined {
  return pickDeclaredRegistryPrefix(registries.registriesByPrefix, registry)
}

/**
 * The prefix the key of a package a `<prefix>:` specifier resolved carries:
 * the one the keys of its registry carry, or `prefix` itself when no declared
 * prefix names that registry. `undefined` when nothing declares `prefix`.
 */
export function keyPrefixForAlias (registries: RegistryContext, prefix: string): string | undefined {
  const registry = registryOfPrefix(registries, prefix)
  if (registry == null) return undefined
  return keyPrefixForRegistry(registries, registry) ?? prefix
}

/**
 * The key this configuration writes for the package of `depPath` in place of
 * `depPath`, when it differs: a plain key of a package whose registry declares
 * a prefix, or a key qualified by a prefix other than the one the keys of its
 * registry carry. A key that is not a registry version, or that is qualified
 * by a prefix nothing declares, is left alone.
 */
export function rekeyDepPath (registries: RegistryContext, depPath: string): DepPath | undefined {
  const { name, version, registryName } = dp.parse(depPath)
  if (name == null || version == null) return undefined
  const currentPrefix = registryName == null
    ? keyPrefixForRegistry(registries, pickRegistryForPackage(registries.registriesByScope, name))
    : keyPrefixForAlias(registries, registryName)
  if (currentPrefix == null || currentPrefix === registryName) return undefined
  return `${name}@${currentPrefix}:${version}${depPath.slice(dp.removeSuffix(depPath).length)}` as DepPath
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
  if (registries.registriesByPrefix == null || Object.keys(registries.registriesByPrefix).length === 0) return undefined
  let outdated: OutdatedRegistryKey | undefined
  for (const key of Object.keys(lockfile.packages ?? {}) as DepPath[]) {
    if (outdated != null && outdated.key < key) continue
    const current = rekeyDepPath(registries, key)
    if (current != null) {
      outdated = { key, current }
    }
  }
  return outdated
}
