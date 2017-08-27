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
    if (dep.dev) {
      shrinkwrap.devDependencies[dep.name] = ref
    } else if (dep.optional) {
      shrinkwrap.optionalDependencies[dep.name] = ref
    } else {
      shrinkwrap.dependencies[dep.name] = ref
    }
    if (!dep.dev) {
      delete shrinkwrap.devDependencies[dep.name]
    }
    if (!dep.optional) {
      delete shrinkwrap.optionalDependencies[dep.name]
    }
    if (dep.dev || dep.optional) {
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
