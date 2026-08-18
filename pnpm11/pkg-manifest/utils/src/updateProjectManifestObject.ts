import { packageManifestLogger } from '@pnpm/core-loggers'
import { isValidPeerRange } from '@pnpm/deps.peer-range'
import {
  DEPENDENCIES_FIELDS,
  DEPENDENCIES_OR_PEER_FIELDS,
  type DependenciesField,
  type DependenciesOrPeersField,
  type ProjectManifest,
  type RangeSpecStyle,
} from '@pnpm/types'
import semver from 'semver'

import { versionWithRangeSpecStyle } from './rangeSpecStyle.js'

export interface PackageSpecObject {
  alias: string
  peer?: boolean
  bareSpecifier?: string
  resolvedVersion?: string
  rangeSpecStyle?: RangeSpecStyle
  saveType?: DependenciesField
}

function getPeerSpecifier (spec: string, resolvedVersion?: string, rangeSpecStyle?: RangeSpecStyle): string {
  if (isValidPeerRange(spec)) return spec

  const rangeFromResolved = resolvedVersion ? createVersionSpecFromResolvedVersion(resolvedVersion, rangeSpecStyle) : null
  return rangeFromResolved ?? '*'
}

export function createVersionSpecFromResolvedVersion (resolvedVersion: string, rangeSpecStyle?: RangeSpecStyle): string | null {
  const parsed = semver.parse(resolvedVersion)
  if (!parsed) return null
  if (parsed.prerelease.length) return resolvedVersion

  return versionWithRangeSpecStyle(resolvedVersion, rangeSpecStyle ?? 'major')
}

export async function updateProjectManifestObject (
  prefix: string,
  packageManifest: ProjectManifest,
  packageSpecs: PackageSpecObject[]
): Promise<ProjectManifest> {
  applyPackageSpecs(packageManifest, packageSpecs)

  packageManifestLogger.debug({
    prefix,
    updated: packageManifest,
  })
  return packageManifest
}

/**
 * The manifest edit {@link updateProjectManifestObject} applies, without
 * announcing it. Separate so a caller that may still discard the edited
 * manifest reports the update only once it commits to it.
 */
export function applyPackageSpecs (
  packageManifest: ProjectManifest,
  packageSpecs: PackageSpecObject[]
): ProjectManifest {
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
            getPeerSpecifier(spec, packageSpec.resolvedVersion, packageSpec.rangeSpecStyle)
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
