import {Resolution} from 'package-store'
import encodeRegistry = require('encode-registry')
import {Shrinkwrap} from 'pnpm-shrinkwrap'
import {Package} from '../types'

export function syncShrinkwrapWithManifest (
  shrinkwrap: Shrinkwrap,
  pkg: Package,
  pkgsToSave: {
    optional: boolean,
    dev: boolean,
    resolution: Resolution,
    absolutePath: string,
    name: string,
  }[]
) {
  shrinkwrap.dependencies = shrinkwrap.dependencies || {}
  shrinkwrap.specifiers = shrinkwrap.specifiers || {}
  shrinkwrap.optionalDependencies = shrinkwrap.optionalDependencies || {}
  shrinkwrap.devDependencies = shrinkwrap.devDependencies || {}

  const deps = pkg.dependencies || {}
  const devDeps = pkg.devDependencies || {}
  const optionalDeps = pkg.optionalDependencies || {}

  const getSpecFromPkg = (depName: string) => deps[depName] || devDeps[depName] || optionalDeps[depName]

  for (const dep of pkgsToSave) {
    const ref = absolutePathToRef(dep.absolutePath, dep.name, dep.resolution, shrinkwrap.registry)
    const isDev = !!devDeps[dep.name]
    const isOptional = !!optionalDeps[dep.name]
    if (isDev) {
      shrinkwrap.devDependencies[dep.name] = ref
    } else if (isOptional) {
      shrinkwrap.optionalDependencies[dep.name] = ref
    } else {
      shrinkwrap.dependencies[dep.name] = ref
    }
    if (!isDev) {
      delete shrinkwrap.devDependencies[dep.name]
    }
    if (!isOptional) {
      delete shrinkwrap.optionalDependencies[dep.name]
    }
    if (isDev || isOptional) {
      delete shrinkwrap.dependencies[dep.name]
    }
    shrinkwrap.specifiers[dep.name] = getSpecFromPkg(dep.name)
  }
}

export function absolutePathToRef (
  absolutePath: string,
  pkgName: string,
  resolution: Resolution,
  standardRegistry: string
) {
  if (resolution.type) return absolutePath

  const registryName = encodeRegistry(standardRegistry)
  if (absolutePath.startsWith(`${registryName}/`)) {
    const ref = absolutePath.replace(`${registryName}/${pkgName}/`, '')
    if (ref.indexOf('/') === -1) return ref
    return absolutePath.replace(`${registryName}/`, '/')
  }
  return absolutePath
}
