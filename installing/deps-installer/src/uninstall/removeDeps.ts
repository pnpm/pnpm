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
    // `Object.hasOwn` rules out `__proto__`, `constructor`, etc. on `opts.saveType`
    // so the dynamic read can never land on `Object.prototype`.
    if (!Object.hasOwn(packageManifest, opts.saveType)) return packageManifest
    const targetDeps = packageManifest[opts.saveType]
    if (targetDeps == null) return packageManifest

    for (const dependency of safeRemovedPackages) {
      if (Object.hasOwn(targetDeps, dependency)) {
        delete targetDeps[dependency]
      }
    }
  } else {
    for (const depField of DEPENDENCIES_FIELDS) {
      const fieldDeps = packageManifest[depField]
      if (!fieldDeps) continue
      for (const dependency of safeRemovedPackages) {
        if (Object.hasOwn(fieldDeps, dependency)) {
          delete fieldDeps[dependency]
        }
      }
    }
  }
  if (packageManifest.peerDependencies != null) {
    const peerDeps = packageManifest.peerDependencies
    for (const removedDependency of safeRemovedPackages) {
      if (Object.hasOwn(peerDeps, removedDependency)) {
        delete peerDeps[removedDependency]
      }
    }
  }
  if (packageManifest.dependenciesMeta != null) {
    const depsMeta = packageManifest.dependenciesMeta
    for (const removedDependency of safeRemovedPackages) {
      if (Object.hasOwn(depsMeta, removedDependency)) {
        delete depsMeta[removedDependency]
      }
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
