import { packageManifestLogger } from '@pnpm/core-loggers'
import {
  DependenciesField,
  DEPENDENCIES_FIELDS,
  ProjectManifest,
} from '@pnpm/types'

export interface PackageSpecObject {
  alias: string
  peer?: boolean
  pref?: string
  saveType?: DependenciesField
}

export async function updateProjectManifestObject (
  prefix: string,
  packageManifest: ProjectManifest,
  packageSpecs: PackageSpecObject[]
): Promise<ProjectManifest> {
  packageSpecs.forEach((packageSpec) => {
    if (packageSpec.saveType) {
      const spec = packageSpec.pref ?? findSpec(packageSpec.alias, packageManifest)
      if (spec) {
        packageManifest[packageSpec.saveType] = packageManifest[packageSpec.saveType] ?? {}
        packageManifest[packageSpec.saveType]![packageSpec.alias] = spec
        DEPENDENCIES_FIELDS.filter((depField) => depField !== packageSpec.saveType).forEach((deptype) => {
          if (packageManifest[deptype]) {
            delete packageManifest[deptype]![packageSpec.alias]
          }
        })
        if (packageSpec.peer === true) {
          packageManifest.peerDependencies = packageManifest.peerDependencies ?? {}
          packageManifest.peerDependencies[packageSpec.alias] = spec
        }
      }
    } else if (packageSpec.pref) {
      const usedDepType = guessDependencyType(packageSpec.alias, packageManifest) ?? 'dependencies'
      packageManifest[usedDepType] = packageManifest[usedDepType] ?? {}
      packageManifest[usedDepType]![packageSpec.alias] = packageSpec.pref
    }
  })

  packageManifestLogger.debug({
    prefix,
    updated: packageManifest,
  })
  return packageManifest
}

function findSpec (alias: string, manifest: ProjectManifest): string | undefined {
  const foundDepType = guessDependencyType(alias, manifest)
  return foundDepType && manifest[foundDepType]![alias]
}

export function guessDependencyType (alias: string, manifest: ProjectManifest): DependenciesField | undefined {
  return DEPENDENCIES_FIELDS
    .find((depField) => Boolean(manifest[depField]?.[alias]))
}
