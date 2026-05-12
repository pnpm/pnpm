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
  // Skip prototype-polluting keys early so they don't reach dynamic property deletes.
  // These are never valid npm package names.
  const safeRemovedPackages = removedPackages.filter((dep) => !isProtoPollutionKey(dep))
  if (opts.saveType) {
    const targetDeps = packageManifest[opts.saveType]
    if (targetDeps == null) return packageManifest

    for (const dependency of safeRemovedPackages) {
      delete targetDeps[dependency]
    }
  } else {
    for (const depField of DEPENDENCIES_FIELDS) {
      const fieldDeps = packageManifest[depField]
      if (!fieldDeps) continue
      for (const dependency of safeRemovedPackages) {
        delete fieldDeps[dependency]
      }
    }
  }
  if (packageManifest.peerDependencies != null) {
    for (const removedDependency of safeRemovedPackages) {
      delete packageManifest.peerDependencies[removedDependency]
    }
  }
  if (packageManifest.dependenciesMeta != null) {
    for (const removedDependency of safeRemovedPackages) {
      delete packageManifest.dependenciesMeta[removedDependency]
    }
  }

  packageManifestLogger.debug({
    prefix: opts.prefix,
    updated: packageManifest,
  })
  return packageManifest
}

function isProtoPollutionKey (key: string): boolean {
  return key === '__proto__' || key === 'constructor' || key === 'prototype'
}
