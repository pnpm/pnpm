import { packageManifestLogger } from '@pnpm/core-loggers'
import { isValidPeerRange } from '@pnpm/deps.peer-range'
import {
  DEPENDENCIES_FIELDS,
  DEPENDENCIES_OR_PEER_FIELDS,
  type DependenciesField,
  type DependenciesOrPeersField,
  type PinnedVersion,
  type ProjectManifest,
} from '@pnpm/types'
import semver from 'semver'

export interface PackageSpecObject {
  alias: string
  peer?: boolean
  bareSpecifier?: string
  resolvedVersion?: string
  pinnedVersion?: PinnedVersion
  saveType?: DependenciesField
}

function getPeerSpecifier (spec: string, resolvedVersion?: string, pinnedVersion?: PinnedVersion): string {
  if (isValidPeerRange(spec)) return spec

  const rangeFromResolved = resolvedVersion ? createVersionSpecFromResolvedVersion(resolvedVersion, pinnedVersion) : null
  return rangeFromResolved ?? '*'
}

export function createVersionSpecFromResolvedVersion (resolvedVersion: string, pinnedVersion?: PinnedVersion): string | null {
  const parsed = semver.parse(resolvedVersion)
  if (!parsed) return null
  if (parsed.prerelease.length) return resolvedVersion

  switch (pinnedVersion ?? 'major') {
    case 'none':
    case 'major':
      return `^${resolvedVersion}`
    case 'minor':
      return `~${resolvedVersion}`
    case 'patch':
      return resolvedVersion
    default:
      return `^${resolvedVersion}`
  }
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
        defineDepEntry(packageManifest[packageSpec.saveType]!, packageSpec.alias, spec)
        for (const deptype of DEPENDENCIES_FIELDS) {
          if (deptype !== packageSpec.saveType) {
            deleteDepEntry(packageManifest[deptype], packageSpec.alias)
          }
        }
        if (packageSpec.peer === true) {
          packageManifest.peerDependencies = packageManifest.peerDependencies ?? {}
          defineDepEntry(
            packageManifest.peerDependencies,
            packageSpec.alias,
            getPeerSpecifier(spec, packageSpec.resolvedVersion, packageSpec.pinnedVersion)
          )
        }
      }
    } else if (packageSpec.bareSpecifier) {
      const usedDepType = guessDependencyType(packageSpec.alias, packageManifest) ?? 'dependencies'
      if (usedDepType !== 'peerDependencies') {
        packageManifest[usedDepType] = packageManifest[usedDepType] ?? {}
        defineDepEntry(packageManifest[usedDepType]!, packageSpec.alias, packageSpec.bareSpecifier)
      }
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
  if (foundDepType == null) return undefined
  const deps = manifest[foundDepType]!
  return Object.hasOwn(deps, alias) ? deps[alias] : undefined
}

export function guessDependencyType (alias: string, manifest: ProjectManifest): DependenciesOrPeersField | undefined {
  return DEPENDENCIES_OR_PEER_FIELDS.find((depField) => {
    const deps = manifest[depField]
    if (deps == null || !Object.hasOwn(deps, alias)) return false
    return deps[alias] === '' || Boolean(deps[alias])
  })
}

/**
 * Write a dependency entry without risking prototype pollution: even when the
 * alias matches a name like `__proto__`, `Object.defineProperty` creates a
 * regular own data property rather than reaching through the setter.
 */
function defineDepEntry (target: Record<string, string>, alias: string, value: string): void {
  Object.defineProperty(target, alias, {
    value,
    enumerable: true,
    writable: true,
    configurable: true,
  })
}

/**
 * Mirror of `defineDepEntry` for deletes. The `Object.hasOwn` guard keeps the
 * `delete` from reaching into the prototype chain when the alias matches an
 * inherited property like `constructor`.
 */
function deleteDepEntry (target: Record<string, string> | undefined, alias: string): void {
  if (target != null && Object.hasOwn(target, alias)) {
    delete target[alias]
  }
}
