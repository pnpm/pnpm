import type {
  PackageExtension,
  ReadPackageHook,
} from '@pnpm/types'
import { packageExtensions as compatPackageExtensions } from '@yarnpkg/extensions'
import { isEmpty } from 'ramda'

import { createOptionalDependenciesRemover } from './createOptionalDependenciesRemover.js'
import { createPackageExtender } from './createPackageExtender.js'
import { createVersionsOverrider, type VersionOverrideWithoutRawSelector } from './createVersionsOverrider.js'

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
  if (!ignoreCompatibilityDb) {
    hooks.push(createPackageExtender(Object.fromEntries(compatPackageExtensions)))
  }
  if (!isEmpty(packageExtensions ?? {})) {
    hooks.push(createPackageExtender(packageExtensions!))
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
  if (hooks.length === 1) {
    return hooks[0]
  }
  const readPackageAndExtend: ReadPackageHook = async (pkg, dir) => {
    let result = pkg
    for (const hook of hooks) {
      // Hooks must run sequentially: each hook sees the manifest produced by the previous one.
      // eslint-disable-next-line no-await-in-loop
      result = await hook(result, dir)
    }
    return result
  }
  return readPackageAndExtend
}
