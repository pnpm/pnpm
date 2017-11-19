import {Dependencies} from '@pnpm/types'
import npa = require('@zkochan/npm-package-arg')
import {PackageSpec} from 'package-store'

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
    optionalDependencies: Dependencies,
    devDependencies: Dependencies,
    existingSpecs: Dependencies,
  }
): PackageSpec[] {
  if (!deps) return []
  return Object.keys(deps).map(pkgName => depToSpec({
    pkgName,
    pkgVersion: deps[pkgName] || opts.existingSpecs[pkgName],
    where: opts.where,
    dev: opts.dev || !!opts.devDependencies[pkgName],
    optional: opts.optional || !!opts.optionalDependencies[pkgName],
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
  const spec = npa.resolve(opts.pkgName, opts.pkgVersion, opts.where)
  spec.dev = opts.dev
  spec.optional = opts.optional
  return spec
}
