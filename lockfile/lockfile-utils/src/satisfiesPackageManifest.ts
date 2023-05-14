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
): { satisfies: boolean, detailedReason?: string } {
  if (!importer) return { satisfies: false, detailedReason: 'no importer' }
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
  if (!equals(existingDeps, specs)) {
    return {
      satisfies: false,
      detailedReason: `specifiers in the lockfile (${JSON.stringify(specs)}) don't match specs in package.json (${JSON.stringify(existingDeps)})`,
    }
  }
  if (importer.publishDirectory !== pkg.publishConfig?.directory) {
    return {
      satisfies: false,
      detailedReason: `"publishDirectory" in the lockfile (${importer.publishDirectory ?? 'undefined'}) doesn't match "publishConfig.directory" in package.json (${pkg.publishConfig?.directory ?? 'undefined'})`,
    }
  }
  if (!equals(pkg.dependenciesMeta ?? {}, importer.dependenciesMeta ?? {})) {
    return {
      satisfies: false,
      detailedReason: `importer dependencies meta (${JSON.stringify(importer.dependenciesMeta)}) doesn't match package manifest dependencies meta (${JSON.stringify(pkg.dependenciesMeta)})`,
    }
  }
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
    if (
      pkgDepNames.length !== Object.keys(importerDeps).length &&
      pkgDepNames.length !== countOfNonLinkedDeps(importerDeps)
    ) {
      return {
        satisfies: false,
        detailedReason: `"${depField}" in the lockfile (${JSON.stringify(importerDeps)}) doesn't match the same field in package.json (${JSON.stringify(pkgDeps)})`,
      }
    }
    for (const depName of pkgDepNames) {
      if (!importerDeps[depName] || importer.specifiers?.[depName] !== pkgDeps[depName]) {
        return {
          satisfies: false,
          detailedReason: `importer ${depField}.${depName} specifier ${importer.specifiers[depName]} don't match package manifest specifier (${pkgDeps[depName]})`,
        }
      }
    }
  }
  return { satisfies: true }
}

function countOfNonLinkedDeps (lockfileDeps: { [depName: string]: string }): number {
  return Object.values(lockfileDeps).filter((ref) => !ref.includes('link:') && !ref.includes('file:')).length
}
