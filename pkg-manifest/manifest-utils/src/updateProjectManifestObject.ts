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
  nodeExecPath?: string | undefined
  peer?: boolean | undefined
  pref?: string | undefined
  saveType?: DependenciesField | undefined
}

export async function updateProjectManifestObject(
  prefix: string,
  packageManifest: ProjectManifest,
  packageSpecs: PackageSpecObject[]
): Promise<ProjectManifest> {
  packageSpecs.forEach((packageSpec) => {
    if (packageSpec.saveType) {
      const spec =
        packageSpec.pref ?? findSpec(packageSpec.alias, packageManifest)
      if (spec) {
        const pm = packageManifest[packageSpec.saveType] ?? {}
        packageManifest[packageSpec.saveType] = pm
        pm[packageSpec.alias] = spec

        DEPENDENCIES_FIELDS.filter(
          (depField: DependenciesField): boolean => {
            return depField !== packageSpec.saveType;
          }
        ).forEach((deptype: DependenciesField): void => {
          if (packageManifest[deptype] != null) {
            delete packageManifest[deptype]?.[packageSpec.alias]
          }
        })
        if (packageSpec.peer === true) {
          packageManifest.peerDependencies =
            packageManifest.peerDependencies ?? {}
          packageManifest.peerDependencies[packageSpec.alias] = spec
        }
      }
    } else if (packageSpec.pref) {
      const usedDepType =
        guessDependencyType(packageSpec.alias, packageManifest) ??
        'dependencies'
      if (usedDepType !== 'peerDependencies') {
        const pm = packageManifest[usedDepType] ?? {}
        packageManifest[usedDepType] = pm
        pm[packageSpec.alias] = packageSpec.pref
      }
    }
    if (packageSpec.nodeExecPath) {
      if (packageManifest.dependenciesMeta == null) {
        packageManifest.dependenciesMeta = {}
      }
      packageManifest.dependenciesMeta[packageSpec.alias] = {
        node: packageSpec.nodeExecPath,
      }
    }
  })

  packageManifestLogger.debug({
    prefix,
    updated: packageManifest,
  })
  return packageManifest
}

function findSpec(
  alias: string,
  manifest: ProjectManifest
): string | undefined {
  const foundDepType = guessDependencyType(alias, manifest)
  return foundDepType && manifest[foundDepType]?.[alias]
}

export function guessDependencyType(
  alias: string,
  manifest: ProjectManifest
): DependenciesOrPeersField | undefined {
  return DEPENDENCIES_OR_PEER_FIELDS.find(
    (depField) =>
      manifest[depField]?.[alias] === '' || Boolean(manifest[depField]?.[alias])
  )
}
