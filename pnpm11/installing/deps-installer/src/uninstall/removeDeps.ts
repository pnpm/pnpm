import { packageManifestLogger } from '@pnpm/core-loggers'
import {
  DEPENDENCIES_FIELDS,
  type DependenciesField,
  type ProjectManifest,
} from '@pnpm/types'

export async function removeDeps (
  packageManifest: ProjectManifest,
  removedPackages: string[],
  opts: {
    saveType?: DependenciesField
    prefix: string
  }
): Promise<ProjectManifest> {
  if (opts.saveType) {
    // `Object.hasOwn` rules out `__proto__`, `constructor`, etc. on `opts.saveType`,
    // so the dynamic read can never land on `Object.prototype`.
    if (Object.hasOwn(packageManifest, opts.saveType)) {
      removeOwnEntries(packageManifest[opts.saveType], removedPackages)
    }
  } else {
    for (const depField of DEPENDENCIES_FIELDS) {
      removeOwnEntries(packageManifest[depField], removedPackages)
    }
  }
  removeOwnEntries(packageManifest.peerDependencies, removedPackages)
  removeOwnEntries(packageManifest.dependenciesMeta, removedPackages)

  packageManifestLogger.debug({
    prefix: opts.prefix,
    updated: packageManifest,
  })
  return packageManifest
}

function removeOwnEntries (target: Record<string, unknown> | undefined, keys: string[]): void {
  if (!target) return
  for (const key of keys) {
    removeOwnEntry(target, key)
  }
}

/**
 * Remove an entry from a dependency-like record by its key, but only when the
 * key is an own property. The `Object.hasOwn` guard keeps the `delete` from
 * reaching into the prototype chain even when the dependency name matches an
 * inherited property like `__proto__` or `constructor`.
 */
function removeOwnEntry (target: Record<string, unknown>, key: string): void {
  if (Object.hasOwn(target, key)) {
    delete target[key]
  }
}
