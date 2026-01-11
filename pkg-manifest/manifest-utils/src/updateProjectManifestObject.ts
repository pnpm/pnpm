import { packageManifestLogger } from '@pnpm/core-loggers'
import { isValidPeerRange } from '@pnpm/semver.peer-range'
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
  bareSpecifier?: string
  saveType?: DependenciesField
}

function normalizePeerSpecifier (spec: string): string {
  if (spec.startsWith('workspace:') || spec.startsWith('catalog:')) return spec

  const protocolMatch = /^([a-z][a-z0-9+.-]*):/i.exec(spec)
  if (!protocolMatch) return spec

  const protocol = protocolMatch[1].toLowerCase()
  if (protocol === 'workspace' || protocol === 'catalog') return spec

  const candidate = extractPeerRange(spec.slice(protocolMatch[0].length))
  return (candidate && isValidPeerRange(candidate)) ? candidate : spec
}

function extractPeerRange (specWithoutProtocol: string): string | null {
  if (!specWithoutProtocol) return null
  if (/^[\^~><=\d]/.test(specWithoutProtocol)) return specWithoutProtocol

  const lastAt = specWithoutProtocol.lastIndexOf('@')
  if (lastAt > 0 && lastAt < specWithoutProtocol.length - 1) {
    return specWithoutProtocol.slice(lastAt + 1)
  }
  return null
}

export async function updateProjectManifestObject (
  prefix: string,
  packageManifest: ProjectManifest,
  packageSpecs: PackageSpecObject[]
): Promise<ProjectManifest> {
  for (const packageSpec of packageSpecs) {
    if (packageSpec.saveType) {
      const spec = packageSpec.bareSpecifier ?? findSpec(packageSpec.alias, packageManifest)
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
          packageManifest.peerDependencies[packageSpec.alias] = normalizePeerSpecifier(spec)
        }
      }
    } else if (packageSpec.bareSpecifier) {
      const usedDepType = guessDependencyType(packageSpec.alias, packageManifest) ?? 'dependencies'
      if (usedDepType !== 'peerDependencies') {
        packageManifest[usedDepType] = packageManifest[usedDepType] ?? {}
        packageManifest[usedDepType]![packageSpec.alias] = packageSpec.bareSpecifier
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
