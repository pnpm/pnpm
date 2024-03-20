import {
  DEPENDENCIES_FIELDS,
  type ProjectManifest,
  type DependenciesField,
  DEPENDENCIES_OR_PEER_FIELDS,
  type DependenciesOrPeersField,
} from '@pnpm/types'
import { packageManifestLogger } from '@pnpm/core-loggers'

export type PackageSpecObject = {
  alias: string
  nodeExecPath?: string | undefined
  peer?: boolean | undefined
  pref?: string | undefined
  saveType?: DependenciesField | undefined
}

export async function updateProjectManifestObject(
  prefix: string,
  packageManifest: ProjectManifest | undefined,
  packageSpecs: PackageSpecObject[]
): Promise<ProjectManifest | undefined> {
  packageSpecs.forEach((packageSpec: PackageSpecObject): void => {
    if (packageSpec.saveType) {
      const spec =
        packageSpec.pref ?? findSpec(packageSpec.alias, packageManifest)

      if (spec) {
        packageManifest = packageManifest ?? {}

        const pm = packageManifest[packageSpec.saveType] ?? {}

        packageManifest[packageSpec.saveType] = pm

        pm[packageSpec.alias] = spec

        DEPENDENCIES_FIELDS.filter(
          (depField: DependenciesField): boolean => {
            return depField !== packageSpec.saveType;
          }
        ).forEach((deptype: DependenciesField): void => {
          if (packageManifest?.[deptype] != null) {
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
        packageManifest = packageManifest ?? {}

        const pm = packageManifest[usedDepType] ?? {}

        packageManifest[usedDepType] = pm

        pm[packageSpec.alias] = packageSpec.pref
      }
    }

    if (packageSpec.nodeExecPath) {
      packageManifest = packageManifest ?? {}

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
  manifest: ProjectManifest | undefined
): string | undefined {
  const foundDepType = guessDependencyType(alias, manifest)

  return foundDepType && manifest?.[foundDepType]?.[alias]
}

export function guessDependencyType(
  alias: string,
  manifest: ProjectManifest | undefined
): DependenciesOrPeersField | undefined {
  return DEPENDENCIES_OR_PEER_FIELDS.find(
    (depField: 'optionalDependencies' | 'dependencies' | 'devDependencies' | 'peerDependencies'): boolean => {
      return manifest?.[depField]?.[alias] === '' || Boolean(manifest?.[depField]?.[alias]);
    }
  )
}
