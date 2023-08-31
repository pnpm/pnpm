import { createBase32Hash } from '@pnpm/crypto.base32-hash'
import { type Registries } from '@pnpm/types'
import encodeRegistry from 'encode-registry'
import semver from 'semver'

export function isAbsolute (dependencyPath: string) {
  return dependencyPath[0] !== '/'
}

export function resolve (
  registries: Registries,
  resolutionLocation: string
) {
  if (!isAbsolute(resolutionLocation)) {
    let registryUrl!: string
    if (resolutionLocation[1] === '@') {
      const slashIndex = resolutionLocation.indexOf('/', 1)
      const scope = resolutionLocation.slice(1, slashIndex !== -1 ? slashIndex : 0)
      registryUrl = registries[scope] || registries.default
    } else {
      registryUrl = registries.default
    }
    const registryDirectory = encodeRegistry(registryUrl)
    return `${registryDirectory}${resolutionLocation}`
  }
  return resolutionLocation
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

export function tryGetPackageId (registries: Registries, relDepPath: string) {
  if (relDepPath[0] !== '/') {
    return null
  }
  const sepIndex = indexOfPeersSuffix(relDepPath)
  if (sepIndex !== -1) {
    return resolve(registries, relDepPath.substring(0, sepIndex))
  }
  const underscoreIndex = relDepPath.indexOf('_', relDepPath.lastIndexOf('/'))
  if (underscoreIndex !== -1) {
    return resolve(registries, relDepPath.slice(0, underscoreIndex))
  }
  return resolve(registries, relDepPath)
}

export function refToAbsolute (
  reference: string,
  pkgName: string,
  registries: Registries
) {
  if (reference.startsWith('link:')) {
    return null
  }
  if (!reference.includes('/') || reference.includes('(') && reference.lastIndexOf('/', reference.indexOf('(')) === -1) {
    const registryName = encodeRegistry(getRegistryByPackageName(registries, pkgName))
    return `${registryName}/${pkgName}/${reference}`
  }
  if (reference[0] !== '/') return reference
  const registryName = encodeRegistry(getRegistryByPackageName(registries, pkgName))
  return `${registryName}${reference}`
}

export function getRegistryByPackageName (registries: Registries, packageName: string) {
  if (packageName[0] !== '@') return registries.default
  const scope = packageName.substring(0, packageName.indexOf('/'))
  return registries[scope] || registries.default
}

export function relative (
  registries: Registries,
  packageName: string,
  absoluteResolutionLoc: string
) {
  const registryName = encodeRegistry(getRegistryByPackageName(registries, packageName))

  if (absoluteResolutionLoc.startsWith(`${registryName}/`)) {
    return absoluteResolutionLoc.slice(absoluteResolutionLoc.indexOf('/'))
  }
  return absoluteResolutionLoc
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
    return `/${pkgName}/${reference}`
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
  const _isAbsolute = isAbsolute(dependencyPath)
  const parts = dependencyPath.split('/')
  if (!_isAbsolute) parts.shift()
  const host = _isAbsolute ? parts.shift() : undefined
  if (parts.length === 0) return {
    host,
    isAbsolute: _isAbsolute,
  }
  const name = parts[0].startsWith('@')
    ? `${parts.shift()}/${parts.shift()}`
    : parts.shift()
  let version = parts.join('/')
  if (version) {
    let peerSepIndex!: number
    let peersSuffix: string | undefined
    if (version.includes('(') && version.endsWith(')')) {
      peerSepIndex = version.indexOf('(')
      if (peerSepIndex !== -1) {
        peersSuffix = version.substring(peerSepIndex)
        version = version.substring(0, peerSepIndex)
      }
    } else {
      peerSepIndex = version.indexOf('_')
      if (peerSepIndex !== -1) {
        peersSuffix = version.substring(peerSepIndex + 1)
        version = version.substring(0, peerSepIndex)
      }
    }
    if (semver.valid(version)) {
      return {
        host,
        isAbsolute: _isAbsolute,
        name,
        peersSuffix,
        version,
      }
    }
  }
  if (!_isAbsolute) throw new Error(`${dependencyPath} is an invalid relative dependency path`)
  return {
    host,
    isAbsolute: _isAbsolute,
  }
}

const MAX_LENGTH_WITHOUT_HASH = 120 - 26 - 1

export function depPathToFilename (depPath: string) {
  let filename = depPathToFilenameUnescaped(depPath).replace(/[\\/:*?"<>|]/g, '+')
  if (filename.includes('(')) {
    filename = filename
      .replace(/(\)\()|\(/g, '_')
      .replace(/\)$/, '')
  }
  if (filename.length > 120 || filename !== filename.toLowerCase() && !filename.startsWith('file+')) {
    return `${filename.substring(0, MAX_LENGTH_WITHOUT_HASH)}_${createBase32Hash(filename)}`
  }
  return filename
}

function depPathToFilenameUnescaped (depPath: string) {
  if (depPath.indexOf('file:') !== 0) {
    if (depPath.startsWith('/')) {
      depPath = depPath.substring(1)
    }
    const index = depPath.lastIndexOf('/', depPath.includes('(') ? depPath.indexOf('(') - 1 : depPath.length)
    return `${depPath.substring(0, index)}@${depPath.slice(index + 1)}`
  }
  return depPath.replace(':', '+')
}

export function createPeersFolderSuffix (peers: Array<{ name: string, version: string }>): string {
  const folderName = peers.map(({ name, version }) => `${name}@${version}`).sort().join(')(')
  return `(${folderName})`
}
