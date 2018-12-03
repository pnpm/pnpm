import encodeRegistry = require('encode-registry')
import semver = require('semver')

export function isAbsolute (dependencyPath: string) {
  return dependencyPath[0] !== '/'
}

export function resolve (
  registryUrl: string,
  resolutionLocation: string
) {
  if (!isAbsolute(resolutionLocation)) {
    const registryDirectory = encodeRegistry(registryUrl)
    return `${registryDirectory}${resolutionLocation}`
  }
  return resolutionLocation
}

export function refToAbsolute (
  reference: string,
  pkgName: string,
  registry: string
) {
  if (reference.startsWith('link:')) {
    return null
  }
  if (reference.indexOf('/') === -1) {
    const registryName = encodeRegistry(registry)
    return `${registryName}/${pkgName}/${reference}`
  }
  if (reference[0] !== '/') return reference
  const registryName = encodeRegistry(registry)
  return `${registryName}${reference}`
}

export function relative (
  standardRegistry: string,
  absoluteResolutionLoc: string
) {
  const registryName = encodeRegistry(standardRegistry)

  if (absoluteResolutionLoc.startsWith(`${registryName}/`) && absoluteResolutionLoc.indexOf('/-/') === -1) {
    return absoluteResolutionLoc.substr(absoluteResolutionLoc.indexOf('/'))
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
  if (reference.indexOf('/') === -1) {
    return `/${pkgName}/${reference}`
  }
  return reference
}

export function parse (dependencyPath: string) {
  if (typeof dependencyPath !== 'string') {
    throw new TypeError(`Expected \`dependencyPath\` to be of type \`string\`, got \`${typeof dependencyPath}\``)
  }
  const _isAbsolute = isAbsolute(dependencyPath)
  const parts = dependencyPath.split('/')
  if (!_isAbsolute) parts.shift()
  const host = _isAbsolute ? parts.shift() : undefined
  const name = parts[0].startsWith('@')
    ? `${parts.shift()}/${parts.shift()}`
    : parts.shift()
  const version = parts.shift()
  if (version && semver.valid(version)) {
    return {
      host,
      isAbsolute: _isAbsolute,
      name,
      version,
    }
  }
  if (!_isAbsolute) throw new Error(`${dependencyPath} is an invalid relative dependency path`)
  return {
    host,
    isAbsolute: _isAbsolute,
  }
}
