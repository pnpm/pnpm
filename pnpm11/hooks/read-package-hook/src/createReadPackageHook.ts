import type {
  PackageExtension,
  ReadPackageHook,
} from '@pnpm/types'
import { packageExtensions as compatPackageExtensions } from '@yarnpkg/extensions'
import { isEmpty } from 'ramda'

import { createOptionalDependenciesRemover } from './createOptionalDependenciesRemover.js'
import { createPackageExtender } from './createPackageExtender.js'
import { createVersionsOverrider, type CreateVersionsOverriderOptions, type VersionOverrideWithoutRawSelector } from './createVersionsOverrider.js'

type PackageExtensionField = 'dependencies' | 'optionalDependencies' | 'peerDependencies' | 'peerDependenciesMeta'

const PACKAGE_EXTENSION_FIELDS: PackageExtensionField[] = [
  'dependencies',
  'optionalDependencies',
  'peerDependencies',
  'peerDependenciesMeta',
]

// pnpm-specific entries not in `@yarnpkg/extensions` yet; keep identical
// to pacquet's `pnpm_compat_package_extensions.json`.
const pnpmCompatPackageExtensions: Array<[string, PackageExtension]> = [
  ['@angular/build@*', { dependencies: { tslib: '^2.3.0' } }],
  ['@nuxt/vite-builder@>=4.0.0 <4.5.0', { dependencies: { unplugin: '^2.3.5' } }],
  ['@nuxt/vite-builder@>=4.5.0', { dependencies: { unplugin: '^3.3.0' } }],
]

export function getEffectivePackageExtensions (
  {
    ignoreCompatibilityDb,
    packageExtensions,
  }: {
    ignoreCompatibilityDb?: boolean
    packageExtensions?: Record<string, PackageExtension>
  }
): Record<string, PackageExtension> | undefined {
  const effectivePackageExtensions: Record<string, PackageExtension> = {}
  if (!ignoreCompatibilityDb) {
    mergePackageExtensions(effectivePackageExtensions, compatPackageExtensions)
    mergePackageExtensions(effectivePackageExtensions, pnpmCompatPackageExtensions)
  }
  if (!isEmpty(packageExtensions ?? {})) {
    mergePackageExtensions(effectivePackageExtensions, Object.entries(packageExtensions!))
  }
  return isEmpty(effectivePackageExtensions) ? undefined : effectivePackageExtensions
}

export function createReadPackageHook (
  {
    catalogs,
    ignoreCompatibilityDb,
    lockfileDir,
    overridesRootDir,
    overrides,
    convergeDeclaredRanges,
    ignoredOptionalDependencies,
    packageExtensions,
    readPackageHook,
  }: {
    catalogs?: CreateVersionsOverriderOptions['catalogs']
    ignoreCompatibilityDb?: boolean
    lockfileDir: string
    /** The directory relative local paths in `overrides` resolve from. Defaults to `lockfileDir`. */
    overridesRootDir?: string
    overrides?: VersionOverrideWithoutRawSelector[]
    convergeDeclaredRanges?: CreateVersionsOverriderOptions['convergeDeclaredRanges']
    ignoredOptionalDependencies?: string[]
    packageExtensions?: Record<string, PackageExtension>
    readPackageHook?: ReadPackageHook[] | ReadPackageHook
  }
): ReadPackageHook | undefined {
  const hooks = collectBaseHooks({
    catalogs,
    packageExtensions,
    readPackageHook,
    overrides,
    overridesRootDir: overridesRootDir ?? lockfileDir,
    convergeDeclaredRanges,
    ignoredOptionalDependencies,
  })

  const dependencyHooks = [...hooks]
  const compatibilityPackageExtensions = getEffectivePackageExtensions({
    ignoreCompatibilityDb,
  })
  if (compatibilityPackageExtensions != null) {
    dependencyHooks.unshift(createPackageExtender(compatibilityPackageExtensions))
  }

  if (dependencyHooks.length === 0) {
    return undefined
  }
  const readPackageAndExtend: ReadPackageHook = async (pkg, dir) => {
    const hooksForManifest = dir == null ? dependencyHooks : hooks
    let result = pkg
    for (const hook of hooksForManifest) {
      // eslint-disable-next-line no-await-in-loop -- each hook must see the manifest produced by the previous one
      result = await hook(result, dir)
    }
    return result
  }
  return readPackageAndExtend
}

function collectBaseHooks (
  {
    catalogs,
    packageExtensions,
    readPackageHook,
    overrides,
    overridesRootDir,
    convergeDeclaredRanges,
    ignoredOptionalDependencies,
  }: {
    catalogs?: CreateVersionsOverriderOptions['catalogs']
    packageExtensions?: Record<string, PackageExtension>
    readPackageHook?: ReadPackageHook[] | ReadPackageHook
    overrides?: VersionOverrideWithoutRawSelector[]
    overridesRootDir: string
    convergeDeclaredRanges?: CreateVersionsOverriderOptions['convergeDeclaredRanges']
    ignoredOptionalDependencies?: string[]
  }
): ReadPackageHook[] {
  const hooks: ReadPackageHook[] = []
  if (!isEmpty(packageExtensions ?? {})) {
    hooks.push(createPackageExtender(packageExtensions!))
  }
  if (Array.isArray(readPackageHook)) {
    hooks.push(...readPackageHook)
  } else if (readPackageHook) {
    hooks.push(readPackageHook)
  }
  if (!isEmpty(overrides ?? {})) {
    hooks.push(createVersionsOverrider(overrides!, overridesRootDir, { catalogs, convergeDeclaredRanges }))
  }
  if (ignoredOptionalDependencies && !isEmpty(ignoredOptionalDependencies)) {
    hooks.push(createOptionalDependenciesRemover(ignoredOptionalDependencies))
  }
  return hooks
}

function mergePackageExtensions (
  target: Record<string, PackageExtension>,
  entries: Iterable<[string, PackageExtension]>
): void {
  for (const [selector, packageExtension] of entries) {
    target[selector] = mergePackageExtension(target[selector], packageExtension)
  }
}

function mergePackageExtension (
  previous: PackageExtension | undefined,
  next: PackageExtension
): PackageExtension {
  if (previous == null) return clonePackageExtension(next)
  const merged = clonePackageExtension(previous)
  for (const field of PACKAGE_EXTENSION_FIELDS) {
    if (next[field] == null) continue
    merged[field] = {
      ...next[field],
      ...merged[field],
    } as never
  }
  return merged
}

function clonePackageExtension (packageExtension: PackageExtension): PackageExtension {
  const cloned: PackageExtension = {}
  for (const field of PACKAGE_EXTENSION_FIELDS) {
    if (packageExtension[field] == null) continue
    cloned[field] = { ...packageExtension[field] } as never
  }
  return cloned
}
