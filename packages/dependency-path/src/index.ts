import { createShortHash } from '@pnpm/crypto.hash'
import { type DepPath, type PkgResolutionId, type Registries, type PkgId, type PkgIdWithPatchHash } from '@pnpm/types'
import semver from 'semver'

export function isAbsolute (dependencyPath: string): boolean {
  return dependencyPath[0] !== '/'
}

export function indexOfDepPathSuffix (depPath: string): { peersIndex: number, patchHashIndex: number } {
  if (!depPath.endsWith(')')) return { peersIndex: -1, patchHashIndex: -1 }
  let open = 1
  for (let i = depPath.length - 2; i >= 0; i--) {
    if (depPath[i] === '(') {
      open--
    } else if (depPath[i] === ')') {
      open++
    } else if (!open) {
      if (depPath.substring(i + 1).startsWith('(patch_hash=')) {
        return {
          patchHashIndex: i + 1,
          peersIndex: depPath.indexOf('(', i + 2),
        }
      }
      return {
        patchHashIndex: -1,
        peersIndex: i + 1,
      }
    }
  }
  return { peersIndex: -1, patchHashIndex: -1 }
}

export interface ParsedDepPath {
  id: string
  peerDepGraphHash: string
}

export function parseDepPath (relDepPath: string): ParsedDepPath {
  const { peersIndex } = indexOfDepPathSuffix(relDepPath)
  if (peersIndex !== -1) {
    return {
      id: relDepPath.substring(0, peersIndex),
      peerDepGraphHash: relDepPath.substring(peersIndex),
    }
  }
  return {
    id: relDepPath,
    peerDepGraphHash: '',
  }
}

export function removeSuffix (relDepPath: string): string {
  const { peersIndex, patchHashIndex } = indexOfDepPathSuffix(relDepPath)
  if (patchHashIndex !== -1) {
    return relDepPath.substring(0, patchHashIndex)
  }
  if (peersIndex !== -1) {
    return relDepPath.substring(0, peersIndex)
  }
  return relDepPath
}

export function getPkgIdWithPatchHash (depPath: DepPath): PkgIdWithPatchHash {
  let pkgId: string = depPath
  const { peersIndex: sepIndex } = indexOfDepPathSuffix(pkgId)
  if (sepIndex !== -1) {
    pkgId = pkgId.substring(0, sepIndex)
  }
  if (pkgId.includes(':')) {
    pkgId = pkgId.substring(pkgId.indexOf('@', 1) + 1)
  }
  return pkgId as PkgIdWithPatchHash
}

export function tryGetPackageId (relDepPath: DepPath): PkgId {
  let pkgId: string = relDepPath
  const { peersIndex, patchHashIndex } = indexOfDepPathSuffix(pkgId)
  const sepIndex = patchHashIndex === -1 ? peersIndex : patchHashIndex
  if (sepIndex !== -1) {
    pkgId = pkgId.substring(0, sepIndex)
  }
  if (pkgId.includes(':')) {
    const newPkgId = pkgId.substring(pkgId.indexOf('@', 1) + 1)
    // TODO: change the format of package ID to always start with the package name.
    // not only in the case of "runtime:"
    if (!newPkgId.startsWith('runtime:')) {
      pkgId = newPkgId
    }
  }
  return pkgId as PkgId
}

export function getRegistryByPackageName (registries: Registries, packageName: string): string {
  if (packageName[0] !== '@') return registries.default
  const scope = packageName.substring(0, packageName.indexOf('/'))
  return registries[scope] || registries.default
}

export function refToRelative (
  reference: string,
  pkgName: string
): DepPath | null {
  if (reference.startsWith('link:')) {
    return null
  }
  if (reference[0] === '@') return reference as DepPath
  const atIndex = reference.indexOf('@')
  if (atIndex === -1) return `${pkgName}@${reference}` as DepPath
  const colonIndex = reference.indexOf(':')
  const bracketIndex = reference.indexOf('(')
  if ((colonIndex === -1 || atIndex < colonIndex) && (bracketIndex === -1 || atIndex < bracketIndex)) return reference as DepPath
  return `${pkgName}@${reference}` as DepPath
}

export interface DependencyPath {
  name?: string
  peerDepGraphHash?: string
  version?: string
  nonSemverVersion?: PkgResolutionId
  patchHash?: string
}

export function parse (dependencyPath: string): DependencyPath {
  // eslint-disable-next-line: strict-type-predicates
  if (typeof dependencyPath !== 'string') {
    throw new TypeError(`Expected \`dependencyPath\` to be of type \`string\`, got \`${
      // eslint-disable-next-line: strict-type-predicates
      dependencyPath === null ? 'null' : typeof dependencyPath
    }\``)
  }
  const sepIndex = dependencyPath.indexOf('@', 1)
  if (sepIndex === -1) {
    return {}
  }
  const name = dependencyPath.substring(0, sepIndex)
  let version = dependencyPath.substring(sepIndex + 1)
  if (version) {
    let peerDepGraphHash: string | undefined
    let patchHash: string | undefined
    const { peersIndex, patchHashIndex } = indexOfDepPathSuffix(version)
    if (peersIndex !== -1 || patchHashIndex !== -1) {
      if (peersIndex === -1) {
        patchHash = version.substring(patchHashIndex)
        version = version.substring(0, patchHashIndex)
      } else if (patchHashIndex === -1) {
        peerDepGraphHash = version.substring(peersIndex)
        version = version.substring(0, peersIndex)
      } else {
        patchHash = version.substring(patchHashIndex, peersIndex)
        peerDepGraphHash = version.substring(peersIndex)
        version = version.substring(0, patchHashIndex)
      }
    }
    if (semver.valid(version)) {
      return {
        name,
        peerDepGraphHash,
        version,
        patchHash,
      }
    }
    return {
      name,
      nonSemverVersion: version as PkgResolutionId,
      peerDepGraphHash,
      patchHash,
    }
  }
  return {}
}

export function depPathToFilename (depPath: string, maxLengthWithoutHash: number): string {
  let filename = depPathToFilenameUnescaped(depPath).replace(/[\\/:*?"<>|#]/g, '+')
  if (filename.includes('(')) {
    filename = filename
      .replace(/\)$/, '')
      .replace(/\)\(|\(|\)/g, '_')
  }
  if (filename.length > maxLengthWithoutHash || filename !== filename.toLowerCase() && !filename.startsWith('file+')) {
    return `${filename.substring(0, maxLengthWithoutHash - 33)}_${createShortHash(filename)}`
  }
  return filename
}

function depPathToFilenameUnescaped (depPath: string): string {
  if (!depPath.startsWith('file:')) {
    if (depPath[0] === '/') {
      depPath = depPath.substring(1)
    }
    const index = depPath.indexOf('@', 1)
    if (index === -1) return depPath
    return `${depPath.substring(0, index)}@${depPath.slice(index + 1)}`
  }
  return depPath.replace(':', '+')
}

// Peer ID or stringified peer dependency graph
export type PeerId = { name: string, version: string } | string

export function createPeerDepGraphHash (peerIds: PeerId[], maxLength: number = 1000): string {
  let dirName = peerIds.map(
    (peerId) => {
      if (typeof peerId !== 'string') {
        return `${peerId.name}@${peerId.version}`
      }
      if (peerId[0] === '/') {
        return peerId.substring(1)
      }
      return peerId
    }
  ).sort().join(')(')
  if (dirName.length > maxLength) {
    dirName = createShortHash(dirName)
  }
  return `(${dirName})`
}
