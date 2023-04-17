import { type ProjectSnapshot } from '@pnpm/lockfile-types'
import {
  DEPENDENCIES_FIELDS,
  type ProjectManifest,
} from '@pnpm/types'
import equals from 'ramda/src/equals'
import pickBy from 'ramda/src/pickBy'
import omit from 'ramda/src/omit'

export function satisfiesPackageManifest (
  opts: {
    autoInstallPeers?: boolean
    excludeLinksFromLockfile?: boolean
  },
  importer: ProjectSnapshot | undefined,
  pkg: ProjectManifest
) {
  if (!importer) return false
  let existingDeps: Record<string, string> = { ...pkg.devDependencies, ...pkg.dependencies, ...pkg.optionalDependencies }
  if (opts?.autoInstallPeers) {
    pkg = {
      ...pkg,
      dependencies: {
        ...omit(Object.keys(existingDeps), pkg.peerDependencies),
        ...pkg.dependencies,
      },
    }
    existingDeps = {
      ...pkg.peerDependencies,
      ...existingDeps,
    }
  }
  const pickNonLinkedDeps = pickBy((spec) => !spec.startsWith('link:'))
  let specs = importer.specifiers
  if (opts?.excludeLinksFromLockfile) {
    existingDeps = pickNonLinkedDeps(existingDeps)
    specs = pickNonLinkedDeps(specs)
  }
  if (
    !equals(existingDeps, specs) ||
    importer.publishDirectory !== pkg.publishConfig?.directory
  ) {
    return false
  }
  if (!equals(pkg.dependenciesMeta ?? {}, importer.dependenciesMeta ?? {})) return false
  for (const depField of DEPENDENCIES_FIELDS) {
    const importerDeps = importer[depField] ?? {}
    let pkgDeps: Record<string, string> = pkg[depField] ?? {}
    if (opts?.excludeLinksFromLockfile) {
      pkgDeps = pickNonLinkedDeps(pkgDeps)
    }

    let pkgDepNames!: string[]
    switch (depField) {
    case 'optionalDependencies':
      pkgDepNames = Object.keys(pkgDeps)
      break
    case 'devDependencies':
      pkgDepNames = Object.keys(pkgDeps)
        .filter((depName) =>
          ((pkg.optionalDependencies == null) || !pkg.optionalDependencies[depName]) &&
            ((pkg.dependencies == null) || !pkg.dependencies[depName])
        )
      break
    case 'dependencies':
      pkgDepNames = Object.keys(pkgDeps)
        .filter((depName) => (pkg.optionalDependencies == null) || !pkg.optionalDependencies[depName])
      break
    default:
      throw new Error(`Unknown dependency type "${depField as string}"`)
    }
    if (pkgDepNames.length !== Object.keys(importerDeps).length &&
      pkgDepNames.length !== countOfNonLinkedDeps(importerDeps)) {
      return false
    }
    for (const depName of pkgDepNames) {
      if (!importerDeps[depName] || importer.specifiers?.[depName] !== pkgDeps[depName]) return false
    }
  }
  return true
}

function countOfNonLinkedDeps (lockfileDeps: { [depName: string]: string }): number {
  return Object.values(lockfileDeps).filter((ref) => !ref.includes('link:') && !ref.includes('file:')).length
}
