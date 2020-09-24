import { Lockfile } from '@pnpm/lockfile-types'
import {
  DEPENDENCIES_FIELDS,
  ProjectManifest,
} from '@pnpm/types'
import R = require('ramda')

export default (lockfile: Lockfile, pkg: ProjectManifest, importerId: string) => {
  const importer = lockfile.importers[importerId]
  if (!importer) return false
  if (!R.equals({ ...pkg.devDependencies, ...pkg.dependencies, ...pkg.optionalDependencies }, importer.specifiers)) {
    return false
  }
  for (const depField of DEPENDENCIES_FIELDS) {
    const importerDeps = importer[depField] ?? {}
    const pkgDeps = pkg[depField] ?? {}

    let pkgDepNames!: string[]
    switch (depField) {
    case 'optionalDependencies':
      pkgDepNames = Object.keys(pkgDeps)
      break
    case 'devDependencies':
      pkgDepNames = Object.keys(pkgDeps)
        .filter((depName) =>
          (!pkg.optionalDependencies || !pkg.optionalDependencies[depName]) &&
            (!pkg.dependencies || !pkg.dependencies[depName])
        )
      break
    case 'dependencies':
      pkgDepNames = Object.keys(pkgDeps)
        .filter((depName) => !pkg.optionalDependencies || !pkg.optionalDependencies[depName])
      break
    default:
      throw new Error(`Unknown dependency type "${depField as string}"`)
    }
    if (pkgDepNames.length !== Object.keys(importerDeps).length &&
      pkgDepNames.length !== countOfNonLinkedDeps(importerDeps)) {
      return false
    }
    for (const depName of pkgDepNames) {
      if (!importerDeps[depName] || importer.specifiers[depName] !== pkgDeps[depName]) return false
    }
  }
  return true
}

function countOfNonLinkedDeps (lockfileDeps: {[depName: string]: string}): number {
  return R.values(lockfileDeps).filter((ref) => !ref.includes('link:') && !ref.includes('file:')).length
}
