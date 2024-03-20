import { packageManifestLogger } from '@pnpm/core-loggers'
import {
  type DependenciesField,
  DEPENDENCIES_FIELDS,
  type ProjectManifest,
} from '@pnpm/types'

export async function removeDeps(
  packageManifest: ProjectManifest | undefined,
  removedPackages: string[],
  opts: {
    saveType?: DependenciesField
    prefix: string
  }
): Promise<ProjectManifest | undefined> {
  if (opts.saveType) {
    if (packageManifest?.[opts.saveType] == null) {
      return packageManifest
    }

    removedPackages.forEach((dependency: string): void => {
      if (typeof opts.saveType === 'string') {
        delete packageManifest[opts.saveType]?.[dependency]
      }
    })
  } else {
    DEPENDENCIES_FIELDS.filter((depField: DependenciesField): boolean => {
      return typeof packageManifest?.[depField] !== 'undefined';
    }).forEach(
      (depField: DependenciesField): void => {
        removedPackages.forEach((dependency: string): void => {
          delete packageManifest?.[depField]?.[dependency]
        })
      }
    )
  }
  if (typeof packageManifest?.peerDependencies !== 'undefined') {
    for (const removedDependency of removedPackages) {
      delete packageManifest.peerDependencies[removedDependency]
    }
  }
  if (typeof packageManifest?.dependenciesMeta !== 'undefined') {
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
