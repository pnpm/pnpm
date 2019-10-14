import { packageManifestLogger } from '@pnpm/core-loggers'
import {
  DEPENDENCIES_FIELDS,
  DependenciesField,
  ImporterManifest,
} from '@pnpm/types'

export type PackageSpecObject = {
  name: string,
  peer?: boolean,
  pref?: string,
  saveType?: DependenciesField | 'peerDependencies',
}

export default async function save (
  prefix: string,
  packageManifest: ImporterManifest,
  packageSpecs: Array<PackageSpecObject>,
  opts?: {
    dryRun?: boolean,
  }
): Promise<ImporterManifest> {
  packageSpecs.forEach((packageSpec) => {
    if (packageSpec.saveType) {
      const spec = packageSpec.pref || findSpec(packageSpec.name, packageManifest as ImporterManifest)
      if (spec) {
        packageManifest[packageSpec.saveType] = packageManifest[packageSpec.saveType] || {}
        packageManifest[packageSpec.saveType]![packageSpec.name] = spec
        DEPENDENCIES_FIELDS.filter((depField) => depField !== packageSpec.saveType).forEach((deptype) => {
          if (packageManifest[deptype]) {
            delete packageManifest[deptype]![packageSpec.name]
          }
        })
        if (packageSpec.peer === true) {
          packageManifest.peerDependencies = packageManifest.peerDependencies || {}
          packageManifest.peerDependencies[packageSpec.name] = spec
        }
      }
    } else if (packageSpec.pref) {
      const usedDepType = guessDependencyType(packageSpec.name, packageManifest as ImporterManifest) || 'dependencies'
      packageManifest[usedDepType] = packageManifest[usedDepType] || {}
      packageManifest[usedDepType]![packageSpec.name] = packageSpec.pref
    }
  })

  packageManifestLogger.debug({
    prefix,
    updated: packageManifest,
  })
  return packageManifest as ImporterManifest
}

function findSpec (depName: string, manifest: ImporterManifest): string | undefined {
  const foundDepType = guessDependencyType(depName, manifest)
  return foundDepType && manifest[foundDepType]![depName]
}

export function guessDependencyType (depName: string, manifest: ImporterManifest): DependenciesField | undefined {
  return DEPENDENCIES_FIELDS
    .find((depField) => Boolean(manifest[depField]?.[depName]))
}
