import { packageExtensions as compatPackageExtensions } from '@yarnpkg/extensions'
import {
  type PackageExtension,
  type PackageManifest,
  type PeerDependencyRules,
  type ProjectManifest,
  type ReadPackageHook,
} from '@pnpm/types'
import isEmpty from 'ramda/src/isEmpty'
import pipeWith from 'ramda/src/pipeWith'
import { createOptionalDependenciesRemover } from './createOptionalDependenciesRemover'
import { createPackageExtender } from './createPackageExtender'
import { createVersionsOverrider } from './createVersionsOverrider'
import { createPeerDependencyPatcher } from './createPeerDependencyPatcher'

export function createReadPackageHook (
  {
    ignoreCompatibilityDb,
    lockfileDir,
    overrides,
    ignoredOptionalDependencies,
    packageExtensions,
    peerDependencyRules,
    readPackageHook,
  }: {
    ignoreCompatibilityDb?: boolean
    lockfileDir: string
    overrides?: Record<string, string>
    ignoredOptionalDependencies?: string[]
    packageExtensions?: Record<string, PackageExtension>
    peerDependencyRules?: PeerDependencyRules
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
  if (
    peerDependencyRules != null &&
    (
      !isEmpty(peerDependencyRules.ignoreMissing) ||
      !isEmpty(peerDependencyRules.allowedVersions) ||
      !isEmpty(peerDependencyRules.allowAny)
    )
  ) {
    hooks.push(createPeerDependencyPatcher(peerDependencyRules))
  }

  if (hooks.length === 0) {
    return undefined
  }
  const readPackageAndExtend = hooks.length === 1
    ? hooks[0]
    : ((pkg: PackageManifest | ProjectManifest, dir: string) => pipeWith(async (f, res) => f(await res, dir), hooks as any)(pkg, dir)) as ReadPackageHook // eslint-disable-line @typescript-eslint/no-explicit-any
  return readPackageAndExtend
}
