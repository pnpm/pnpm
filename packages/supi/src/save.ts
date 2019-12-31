import { packageManifestLogger } from '@pnpm/core-loggers'
import {
  DEPENDENCIES_FIELDS,
  DependenciesField,
  ProjectManifest,
} from '@pnpm/types'

export type PackageSpecObject = {
  alias: string,
  peer?: boolean,
  pref?: string,
  saveType?: DependenciesField,
}

export default async function save (
  prefix: string,
  packageManifest: ProjectManifest,
  packageSpecs: Array<PackageSpecObject>,
  opts?: {
    dryRun?: boolean,
  }
): Promise<ProjectManifest> {
  packageSpecs.forEach((packageSpec) => {
    if (packageSpec.saveType) {
      const spec = packageSpec.pref || findSpec(packageSpec.alias, packageManifest as ProjectManifest)
      if (spec) {
        packageManifest[packageSpec.saveType] = packageManifest[packageSpec.saveType] || {}
        packageManifest[packageSpec.saveType]![packageSpec.alias] = spec
        DEPENDENCIES_FIELDS.filter((depField) => depField !== packageSpec.saveType).forEach((deptype) => {
          if (packageManifest[deptype]) {
            delete packageManifest[deptype]![packageSpec.alias]
          }
        })
        if (packageSpec.peer === true) {
          packageManifest.peerDependencies = packageManifest.peerDependencies || {}
          packageManifest.peerDependencies[packageSpec.alias] = spec
        }
      }
    } else if (packageSpec.pref) {
      const usedDepType = guessDependencyType(packageSpec.alias, packageManifest as ProjectManifest) || 'dependencies'
      packageManifest[usedDepType] = packageManifest[usedDepType] || {}
      packageManifest[usedDepType]![packageSpec.alias] = packageSpec.pref
    }
  })

  packageManifestLogger.debug({
    prefix,
    updated: packageManifest,
  })
  return packageManifest as ProjectManifest
}

function findSpec (alias: string, manifest: ProjectManifest): string | undefined {
  const foundDepType = guessDependencyType(alias, manifest)
  return foundDepType && manifest[foundDepType]![alias]
}

export function guessDependencyType (alias: string, manifest: ProjectManifest): DependenciesField | undefined {
  return DEPENDENCIES_FIELDS
    .find((depField) => Boolean(manifest[depField]?.[alias]))
}
