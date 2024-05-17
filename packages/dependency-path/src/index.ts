import { createBase32Hash } from '@pnpm/crypto.base32-hash'
import { type DepPath, type PkgResolutionId, type Registries, type PkgId } from '@pnpm/types'
import semver from 'semver'

export function isAbsolute (dependencyPath: string): boolean {
  return dependencyPath[0] !== '/'
}

export function indexOfPeersSuffix (depPath: string): number {
  if (!depPath.endsWith(')')) return -1
  let open = 1
  for (let i = depPath.length - 2; i >= 0; i--) {
    if (depPath[i] === '(') {
      open--
    } else if (depPath[i] === ')') {
      open++
    } else if (!open) {
      if (depPath.substring(i + 1).startsWith('(patch_hash=')) {
        return depPath.indexOf('(', i + 2)
      }
      return i + 1
    }
  }
  return -1
}

export interface ParsedDepPath {
  id: string
  peersSuffix: string
}

export function parseDepPath (relDepPath: string): ParsedDepPath {
  const sepIndex = indexOfPeersSuffix(relDepPath)
  if (sepIndex !== -1) {
    return {
      id: relDepPath.substring(0, sepIndex),
      peersSuffix: relDepPath.substring(sepIndex),
    }
  }
  return {
    id: relDepPath,
    peersSuffix: '',
  }
}

export function removePeersSuffix (relDepPath: string): string {
  const sepIndex = indexOfPeersSuffix(relDepPath)
  if (sepIndex !== -1) {
    return relDepPath.substring(0, sepIndex)
  }
  return relDepPath
}

export function tryGetPackageId (relDepPath: DepPath): PkgId {
  let pkgId: string = relDepPath
  const sepIndex = indexOfPeersSuffix(pkgId)
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
    let peerSepIndex!: number
    let peersSuffix: string | undefined
    if (version.includes('(') && version.endsWith(')')) {
      peerSepIndex = version.indexOf('(')
      if (peerSepIndex !== -1) {
        peersSuffix = version.substring(peerSepIndex)
        version = version.substring(0, peerSepIndex)
      }
    }
    if (semver.valid(version)) {
      return {
        name,
        peersSuffix,
        version,
      }
    }
    return {
      name,
      nonSemverVersion: version as PkgResolutionId,
      peersSuffix,
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

export function createPeersDirSuffix (peerIds: PeerId[]): string {
  const dirName = peerIds.map(
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
  return `(${dirName})`
}
