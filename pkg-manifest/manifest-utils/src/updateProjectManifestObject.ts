import { packageManifestLogger } from '@pnpm/core-loggers'
import {
  type DependenciesOrPeersField,
  type DependenciesField,
  DEPENDENCIES_FIELDS,
  DEPENDENCIES_OR_PEER_FIELDS,
  type ProjectManifest,
} from '@pnpm/types'

export interface PackageSpecObject {
  alias: string
  nodeExecPath?: string
  peer?: boolean
  pref?: string
  saveType?: DependenciesField
}

export async function updateProjectManifestObject (
  prefix: string,
  packageManifest: ProjectManifest,
  packageSpecs: PackageSpecObject[]
): Promise<ProjectManifest> {
  for (const packageSpec of packageSpecs) {
    if (packageSpec.saveType) {
      const spec = packageSpec.pref ?? findSpec(packageSpec.alias, packageManifest)
      if (spec) {
        packageManifest[packageSpec.saveType] = packageManifest[packageSpec.saveType] ?? {}
        packageManifest[packageSpec.saveType]![packageSpec.alias] = spec
        for (const deptype of DEPENDENCIES_FIELDS) {
          if (deptype !== packageSpec.saveType) {
            delete packageManifest[deptype]?.[packageSpec.alias]
          }
        }
        if (packageSpec.peer === true) {
          packageManifest.peerDependencies = packageManifest.peerDependencies ?? {}
          packageManifest.peerDependencies[packageSpec.alias] = spec
        }
      }
    } else if (packageSpec.pref) {
      const usedDepType = guessDependencyType(packageSpec.alias, packageManifest) ?? 'dependencies'
      if (usedDepType !== 'peerDependencies') {
        packageManifest[usedDepType] = packageManifest[usedDepType] ?? {}
        packageManifest[usedDepType]![packageSpec.alias] = packageSpec.pref
      }
    }
    if (packageSpec.nodeExecPath) {
      if (packageManifest.dependenciesMeta == null) {
        packageManifest.dependenciesMeta = {}
      }
      packageManifest.dependenciesMeta[packageSpec.alias] = { node: packageSpec.nodeExecPath }
    }
  }

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

export function guessDependencyType (alias: string, manifest: ProjectManifest): DependenciesOrPeersField | undefined {
  return DEPENDENCIES_OR_PEER_FIELDS
    .find((depField) => manifest[depField]?.[alias] === '' || Boolean(manifest[depField]?.[alias]))
}
