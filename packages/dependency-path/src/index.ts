import crypto from 'crypto'
import { Registries } from '@pnpm/types'
import encodeRegistry from 'encode-registry'
import { base32 } from 'rfc4648'
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

export function tryGetPackageId (registries: Registries, relDepPath: string) {
  if (relDepPath[0] !== '/') {
    return null
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
  if (!reference.includes('/')) {
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
) {
  if (reference.startsWith('link:')) {
    return null
  }
  if (reference.startsWith('file:')) {
    return reference
  }
  if (!reference.includes('/')) {
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
  const name = parts[0].startsWith('@')
    ? `${parts.shift()}/${parts.shift()}` // eslint-disable-line @typescript-eslint/restrict-template-expressions
    : parts.shift()
  let version = parts.shift()
  if (version) {
    const underscoreIndex = version.indexOf('_')
    let peersSuffix: string | undefined
    if (underscoreIndex !== -1) {
      peersSuffix = version.substring(underscoreIndex + 1)
      version = version.substring(0, underscoreIndex)
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

export function depPathToFilename (depPath: string) {
  const filename = depPathToFilenameUnescaped(depPath).replace(/\//g, '+')
  if (filename.length > 120 || filename !== filename.toLowerCase() && !filename.startsWith('file+')) {
    return `${filename.substring(0, 50)}_${createBase32Hash(filename)}`
  }
  return filename
}

function depPathToFilenameUnescaped (depPath: string) {
  if (depPath.indexOf('file:') !== 0) {
    if (depPath.startsWith('/')) {
      depPath = depPath.substring(1)
    }
    const index = depPath.lastIndexOf('/')
    return `${depPath.substring(0, index)}@${depPath.slice(index + 1)}`
  }
  return depPath.replace(':', '+')
}

export function createPeersFolderSuffix (peers: Array<{name: string, version: string}>): string {
  const folderName = peers.map(({ name, version }) => `${name.replace('/', '+')}@${version}`).sort().join('+')

  // We don't want the folder name to get too long.
  // Otherwise, an ENAMETOOLONG error might happen.
  // see: https://github.com/pnpm/pnpm/issues/977
  //
  // A bigger limit might be fine but the base32 encoded md5 hash will be 26 symbols,
  // so for consistency's sake, we go with 26.
  if (folderName.length > 26) {
    return `_${createBase32Hash(folderName)}`
  }
  return `_${folderName}`
}

function createBase32Hash (str: string): string {
  return base32.stringify(crypto.createHash('md5').update(str).digest()).replace(/(=+)$/, '').toLowerCase()
}
