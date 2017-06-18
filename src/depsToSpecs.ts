import npa = require('npm-package-arg')
import {Dependencies} from './types'
import {PackageSpec} from './resolve'

export default function (
  deps: Dependencies,
  opts: {
    where: string,
    optionalDependencies: Dependencies,
    devDependencies: Dependencies,
  }
): PackageSpec[] {
  if (!deps) return []
  return Object.keys(deps).map(pkgName => depToSpec({
    pkgName,
    pkgVersion: deps[pkgName],
    where: opts.where,
    dev: !!opts.devDependencies[pkgName],
    optional: !!opts.optionalDependencies[pkgName],
  }))
}

export function similarDepsToSpecs (
  deps: Dependencies,
  opts: {
    where: string,
    dev: boolean,
    optional: boolean,
    existingSpecs: Dependencies,
  }
): PackageSpec[] {
  if (!deps) return []
  return Object.keys(deps).map(pkgName => depToSpec({
    pkgName,
    pkgVersion: deps[pkgName] || opts.existingSpecs[pkgName],
    where: opts.where,
    dev: opts.dev,
    optional: opts.optional,
  }))
}

function depToSpec (
  opts: {
    pkgName: string,
    pkgVersion: string,
    where: string,
    dev: boolean,
    optional: boolean,
  }
): PackageSpec {
  const raw = `${opts.pkgName}@${opts.pkgVersion}`
  const spec = npa(raw, opts.where)
  spec.dev = opts.dev
  spec.optional = opts.optional
  return spec
}
