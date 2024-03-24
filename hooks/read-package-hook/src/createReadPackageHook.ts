import isEmpty from 'ramda/src/isEmpty'
import pipeWith from 'ramda/src/pipeWith'
import { packageExtensions as compatPackageExtensions } from '@yarnpkg/extensions'

import type {
  PackageExtension,
  PackageManifest,
  PeerDependencyRules,
  ProjectManifest,
  ReadPackageHook,
} from '@pnpm/types'

import { createPackageExtender } from './createPackageExtender.js'
import { createVersionsOverrider } from './createVersionsOverrider.js'
import { createPeerDependencyPatcher } from './createPeerDependencyPatcher.js'

export function createReadPackageHook({
  ignoreCompatibilityDb,
  lockfileDir,
  overrides,
  packageExtensions,
  peerDependencyRules,
  readPackageHook,
}: {
  ignoreCompatibilityDb?: boolean | undefined
  lockfileDir: string
  overrides?: Record<string, string> | undefined
  packageExtensions?: Record<string, PackageExtension> | undefined
  peerDependencyRules?: PeerDependencyRules | undefined
  readPackageHook?: ReadPackageHook[] | ReadPackageHook | undefined
}): ReadPackageHook | undefined {
  const hooks: ReadPackageHook[] = []

  if (!ignoreCompatibilityDb) {
    hooks.push(
      createPackageExtender(Object.fromEntries(compatPackageExtensions))
    )
  }

  if (!isEmpty(packageExtensions ?? {})) {
    hooks.push(createPackageExtender(packageExtensions ?? {}))
  }

  if (Array.isArray(readPackageHook)) {
    hooks.push(...readPackageHook)
  } else if (readPackageHook) {
    hooks.push(readPackageHook)
  }

  if (typeof overrides !== 'undefined' && !isEmpty(overrides)) {
    hooks.push(createVersionsOverrider(overrides, lockfileDir))
  }

  if (
    peerDependencyRules != null &&
    (!isEmpty(peerDependencyRules.ignoreMissing) ||
      !isEmpty(peerDependencyRules.allowedVersions) ||
      !isEmpty(peerDependencyRules.allowAny))
  ) {
    hooks.push(createPeerDependencyPatcher(peerDependencyRules))
  }

  if (hooks.length === 0) {
    return undefined
  }

  return hooks.length === 1
    ? hooks[0]
    : (pkg: PackageManifest | ProjectManifest | undefined, dir?: string | undefined): ProjectManifest | PackageManifest | Promise<ProjectManifest | PackageManifest> | undefined => {
      return pipeWith(
        async (f: (arg: unknown, dir: string | undefined) => unknown, res: () => Promise<unknown>) => {
          return f(await res, dir);
        },
        // @ts-ignore
        hooks
      )(
        pkg,
        dir
      );
    }
}
