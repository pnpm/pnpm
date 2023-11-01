import { type ProjectSnapshot } from '@pnpm/lockfile-types'
import {
  DEPENDENCIES_FIELDS,
  type ProjectManifest,
} from '@pnpm/types'
import equals from 'ramda/src/equals'
import mapValues from 'ramda/src/map'
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
        .filter((depName) => !pkg.optionalDependencies?.[depName] && !pkg.dependencies?.[depName])
      break
    case 'dependencies':
      pkgDepNames = Object.keys(pkgDeps)
        .filter((depName) => !pkg.optionalDependencies?.[depName])
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
        detailedReason: `"${depField}" in the lockfile (${JSON.stringify(mapValues(({ specifier }) => specifier, importerDeps))}) doesn't match the same field in package.json (${JSON.stringify(pkgDeps)})`,
      }
    }
    for (const depName of pkgDepNames) {
      if (!importerDeps[depName] || importerDeps[depName].specifier !== pkgDeps[depName]) {
        return {
          satisfies: false,
          detailedReason: `specifier in the lockfile for "${depName}" in "${depField}" (${importerDeps[depName].specifier}) don't match the spec in package.json (${pkgDeps[depName]})`,
        }
      }
    }
  }
  return { satisfies: true }
}

function countOfNonLinkedDeps (lockfileDeps: { [depName: string]: { version: string, specifier: string } }): number {
  return Object.values(lockfileDeps).filter(({ version: ref }) => !ref.includes('link:') && !ref.includes('file:')).length
}
