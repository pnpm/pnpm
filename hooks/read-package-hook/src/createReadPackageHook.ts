import type {
  PackageExtension,
  PackageManifest,
  ProjectManifest,
  ReadPackageHook,
} from '@pnpm/types'
import { packageExtensions as compatPackageExtensions } from '@yarnpkg/extensions'
import { isEmpty, pipeWith } from 'ramda'

import { createOptionalDependenciesRemover } from './createOptionalDependenciesRemover.js'
import { createPackageExtender } from './createPackageExtender.js'
import { createVersionsOverrider, type VersionOverrideWithoutRawSelector } from './createVersionsOverrider.js'

type PackageExtensionField = 'dependencies' | 'optionalDependencies' | 'peerDependencies' | 'peerDependenciesMeta'

const PACKAGE_EXTENSION_FIELDS: PackageExtensionField[] = [
  'dependencies',
  'optionalDependencies',
  'peerDependencies',
  'peerDependenciesMeta',
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
  }
  if (!isEmpty(packageExtensions ?? {})) {
    mergePackageExtensions(effectivePackageExtensions, Object.entries(packageExtensions!))
  }
  return isEmpty(effectivePackageExtensions) ? undefined : effectivePackageExtensions
}

export function createReadPackageHook (
  {
    ignoreCompatibilityDb,
    lockfileDir,
    overrides,
    ignoredOptionalDependencies,
    packageExtensions,
    readPackageHook,
  }: {
    ignoreCompatibilityDb?: boolean
    lockfileDir: string
    overrides?: VersionOverrideWithoutRawSelector[]
    ignoredOptionalDependencies?: string[]
    packageExtensions?: Record<string, PackageExtension>
    readPackageHook?: ReadPackageHook[] | ReadPackageHook
  }
): ReadPackageHook | undefined {
  const hooks: ReadPackageHook[] = []
  const effectivePackageExtensions = getEffectivePackageExtensions({
    ignoreCompatibilityDb,
    packageExtensions,
  })
  if (effectivePackageExtensions != null) {
    hooks.push(createPackageExtender(effectivePackageExtensions))
  }
  if (Array.isArray(readPackageHook)) {
    hooks.push(...readPackageHook)
  } else if (readPackageHook) {
    hooks.push(readPackageHook)
  }
  if (!isEmpty(overrides ?? {})) {
    hooks.push(createVersionsOverrider(overrides!, lockfileDir))
  }
  if (ignoredOptionalDependencies && !isEmpty(ignoredOptionalDependencies)) {
    hooks.push(createOptionalDependenciesRemover(ignoredOptionalDependencies))
  }

  if (hooks.length === 0) {
    return undefined
  }
  const readPackageAndExtend = hooks.length === 1
    ? hooks[0]
    : ((pkg: PackageManifest | ProjectManifest, dir: string) => pipeWith(async (f, res) => f(await res, dir), hooks as any)(pkg, dir)) as ReadPackageHook // eslint-disable-line @typescript-eslint/no-explicit-any
  return readPackageAndExtend
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
