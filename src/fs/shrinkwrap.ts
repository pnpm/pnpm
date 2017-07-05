import {Resolution, getRegistryName} from 'package-store'

export function shortIdToFullId (
  shortId: string,
  registry: string
) {
  if (shortId[0] === '/') {
    const registryName = getRegistryName(registry)
    return `${registryName}${shortId}`
  }
  return shortId
}

export function getPkgShortId (
  reference: string,
  pkgName: string
) {
  if (reference.indexOf('/') === -1) {
    return `/${pkgName}/${reference}`
  }
  return reference
}

export function getPkgId (
  reference: string,
  pkgName: string,
  registry: string
) {
  if (reference.indexOf('/') === -1) {
    const registryName = getRegistryName(registry)
    return `${registryName}/${pkgName}/${reference}`
  }
  return reference
}

export function pkgIdToRef (
  pkgId: string,
  pkgName: string,
  resolution: Resolution,
  standardRegistry: string
) {
  if (resolution.type) return pkgId

  const registryName = getRegistryName(standardRegistry)
  if (pkgId.startsWith(`${registryName}/`)) {
    const ref = pkgId.replace(`${registryName}/${pkgName}/`, '')
    if (ref.indexOf('/') === -1) return ref
    return pkgId.replace(`${registryName}/`, '/')
  }
  return pkgId
}

export function pkgShortId (
  pkgId: string,
  standardRegistry: string
) {
  const registryName = getRegistryName(standardRegistry)

  if (pkgId.startsWith(`${registryName}/`)) {
    return pkgId.substr(pkgId.indexOf('/'))
  }
  return pkgId
}
