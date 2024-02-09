import { createBase32Hash } from '@pnpm/crypto.base32-hash'
import { type Registries } from '@pnpm/types'
import semver from 'semver'

export function isAbsolute (dependencyPath: string) {
  return dependencyPath[0] !== '/'
}

export function indexOfPeersSuffix (depPath: string) {
  if (!depPath.endsWith(')')) return -1
  let open = true
  for (let i = depPath.length - 2; i >= 0; i--) {
    if (depPath[i] === '(') {
      open = false
    } else if (depPath[i] === ')') {
      if (open) return -1
      open = true
    } else if (!open) {
      return i + 1
    }
  }
  return -1
}

export function tryGetPackageId (relDepPath: string) {
  if (relDepPath[0] !== '/') {
    return null
  }
  const sepIndex = indexOfPeersSuffix(relDepPath)
  if (sepIndex !== -1) {
    return relDepPath.substring(0, sepIndex)
  }
  return relDepPath
}

export function getRegistryByPackageName (registries: Registries, packageName: string) {
  if (packageName[0] !== '@') return registries.default
  const scope = packageName.substring(0, packageName.indexOf('/'))
  return registries[scope] || registries.default
}

export function refToRelative (
  reference: string,
  pkgName: string
): string | null {
  if (reference.startsWith('link:')) {
    return null
  }
  if (reference.startsWith('file:')) {
    return reference
  }
  if (!reference.includes('/') || reference.includes('(') && reference.lastIndexOf('/', reference.indexOf('(')) === -1) {
    return `/${pkgName}@${reference}`
  }
  return reference
}

export function parse (dependencyPath: string) {
  // eslint-disable-next-line: strict-type-predicates
  if (typeof dependencyPath !== 'string') {
    throw new TypeError(`Expected \`dependencyPath\` to be of type \`string\`, got \`${
      // eslint-disable-next-line: strict-type-predicates
      dependencyPath === null ? 'null' : typeof dependencyPath
    }\``)
  }
  const sepIndex = dependencyPath.indexOf('@', 2)
  if (sepIndex === -1) {
    return {}
  }
  const name = dependencyPath.substring(1, sepIndex)
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
  }
  return {
  }
}

const MAX_LENGTH_WITHOUT_HASH = 120 - 26 - 1

export function depPathToFilename (depPath: string) {
  let filename = depPathToFilenameUnescaped(depPath).replace(/[\\/:*?"<>|]/g, '+')
  if (filename.includes('(')) {
    filename = filename
      .replace(/\)$/, '')
      .replace(/(\)\()|\(|\)/g, '_')
  }
  if (filename.length > 120 || filename !== filename.toLowerCase() && !filename.startsWith('file+')) {
    return `${filename.substring(0, MAX_LENGTH_WITHOUT_HASH)}_${createBase32Hash(filename)}`
  }
  return filename
}

function depPathToFilenameUnescaped (depPath: string) {
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
