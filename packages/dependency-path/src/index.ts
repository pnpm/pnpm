import { createBase32Hash } from '@pnpm/crypto.base32-hash'
import { type DepPath, type PkgResolutionId, type Registries, type PkgId, type PkgIdWithPatchHash } from '@pnpm/types'
import semver from 'semver'

export function isAbsolute (dependencyPath: string): boolean {
  return dependencyPath[0] !== '/'
}

export function indexOfPeersSuffix (depPath: string): { peersIndex: number, patchHashIndex: number } {
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
  peersSuffix: string
}

export function parseDepPath (relDepPath: string): ParsedDepPath {
  const { peersIndex } = indexOfPeersSuffix(relDepPath)
  if (peersIndex !== -1) {
    return {
      id: relDepPath.substring(0, peersIndex),
      peersSuffix: relDepPath.substring(peersIndex),
    }
  }
  return {
    id: relDepPath,
    peersSuffix: '',
  }
}

export function removeSuffix (relDepPath: string): string {
  const { peersIndex, patchHashIndex } = indexOfPeersSuffix(relDepPath)
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
  const { peersIndex: sepIndex } = indexOfPeersSuffix(pkgId)
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
  const { peersIndex, patchHashIndex } = indexOfPeersSuffix(pkgId)
  const sepIndex = patchHashIndex === -1 ? peersIndex : patchHashIndex
  if (sepIndex !== -1) {
    pkgId = pkgId.substring(0, sepIndex)
  }
  if (pkgId.includes(':')) {
    pkgId = pkgId.substring(pkgId.indexOf('@', 1) + 1)
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
  if (reference.startsWith('@')) return reference as DepPath
  const atIndex = reference.indexOf('@')
  if (atIndex === -1) return `${pkgName}@${reference}` as DepPath
  const colonIndex = reference.indexOf(':')
  const bracketIndex = reference.indexOf('(')
  if ((colonIndex === -1 || atIndex < colonIndex) && (bracketIndex === -1 || atIndex < bracketIndex)) return reference as DepPath
  return `${pkgName}@${reference}` as DepPath
}

export interface DependencyPath {
  name?: string
  peersSuffix?: string
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
    let peersSuffix: string | undefined
    let patchHash: string | undefined
    const { peersIndex, patchHashIndex } = indexOfPeersSuffix(version)
    if (peersIndex !== -1 || patchHashIndex !== -1) {
      if (peersIndex === -1) {
        patchHash = version.substring(patchHashIndex)
        version = version.substring(0, patchHashIndex)
      } else if (patchHashIndex === -1) {
        peersSuffix = version.substring(peersIndex)
        version = version.substring(0, peersIndex)
      } else {
        patchHash = version.substring(patchHashIndex, peersIndex)
        peersSuffix = version.substring(peersIndex)
        version = version.substring(0, patchHashIndex)
      }
    }
    if (semver.valid(version)) {
      return {
        name,
        peersSuffix,
        version,
        patchHash,
      }
    }
    return {
      name,
      nonSemverVersion: version as PkgResolutionId,
      peersSuffix,
      patchHash,
    }
  }
  return {}
}

export function depPathToFilename (depPath: string, maxLengthWithoutHash: number): string {
  let filename = depPathToFilenameUnescaped(depPath).replace(/[\\/:*?"<>|]/g, '+')
  if (filename.includes('(')) {
    filename = filename
      .replace(/\)$/, '')
      .replace(/(\)\()|\(|\)/g, '_')
  }
  if (filename.length > maxLengthWithoutHash || filename !== filename.toLowerCase() && !filename.startsWith('file+')) {
    return `${filename.substring(0, maxLengthWithoutHash - 27)}_${createBase32Hash(filename)}`
  }
  return filename
}

function depPathToFilenameUnescaped (depPath: string): string {
  if (depPath.indexOf('file:') !== 0) {
    if (depPath[0] === '/') {
      depPath = depPath.substring(1)
    }
    const index = depPath.indexOf('@', 1)
    if (index === -1) return depPath
    return `${depPath.substring(0, index)}@${depPath.slice(index + 1)}`
  }
  return depPath.replace(':', '+')
}

export type PeerId = { name: string, version: string } | string

export function createPeersDirSuffix (peerIds: PeerId[], maxLength: number = 1000): string {
  let dirName = peerIds.map(
    (peerId) => {
      if (typeof peerId !== 'string') {
        return `${peerId.name}@${peerId.version}`
      }
      if (peerId.startsWith('/')) {
        return peerId.substring(1)
      }
      return peerId
    }
  ).sort().join(')(')
  if (dirName.length > maxLength) {
    dirName = createBase32Hash(dirName)
  }
  return `(${dirName})`
}
