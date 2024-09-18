import { packageManifestLogger } from '@pnpm/core-loggers'
import {
  type DependenciesField,
  DEPENDENCIES_FIELDS,
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
    if (packageManifest[opts.saveType] == null) return packageManifest

    for (const dependency of removedPackages) {
      delete packageManifest[opts.saveType as DependenciesField]![dependency]
    }
  } else {
    for (const depField of DEPENDENCIES_FIELDS) {
      if (!packageManifest[depField]) continue
      for (const dependency of removedPackages) {
        delete packageManifest[depField]![dependency]
      }
    }
  }
  if (packageManifest.peerDependencies != null) {
    for (const removedDependency of removedPackages) {
      delete packageManifest.peerDependencies[removedDependency]
    }
  }
  if (packageManifest.dependenciesMeta != null) {
    for (const removedDependency of removedPackages) {
      delete packageManifest.dependenciesMeta[removedDependency]
    }
  }

  packageManifestLogger.debug({
    prefix: opts.prefix,
    updated: packageManifest,
  })
  return packageManifest
}
