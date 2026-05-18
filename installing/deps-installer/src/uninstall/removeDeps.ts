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
    if (!Object.hasOwn(packageManifest, opts.saveType)) return packageManifest
    const targetDeps = packageManifest[opts.saveType]
    if (targetDeps == null) return packageManifest

    for (const dependency of removedPackages) {
      removeOwnEntry(targetDeps, dependency)
    }
  } else {
    for (const depField of DEPENDENCIES_FIELDS) {
      const fieldDeps = packageManifest[depField]
      if (!fieldDeps) continue
      for (const dependency of removedPackages) {
        removeOwnEntry(fieldDeps, dependency)
      }
    }
  }
  if (packageManifest.peerDependencies != null) {
    const peerDeps = packageManifest.peerDependencies
    for (const removedDependency of removedPackages) {
      removeOwnEntry(peerDeps, removedDependency)
    }
  }
  if (packageManifest.dependenciesMeta != null) {
    const depsMeta = packageManifest.dependenciesMeta
    for (const removedDependency of removedPackages) {
      removeOwnEntry(depsMeta, removedDependency)
    }
  }

  packageManifestLogger.debug({
    prefix: opts.prefix,
    updated: packageManifest,
  })
  return packageManifest
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
