import * as dp from '@pnpm/dependency-path'
import { type ProjectSnapshot } from '@pnpm/lockfile.types'
import {
  DEPENDENCIES_FIELDS,
  type ProjectManifest,
} from '@pnpm/types'
import { equals, pickBy, omit } from 'ramda'
import semver from 'semver'
import { type Diff, diffFlatRecords, isEqual } from './diffFlatRecords.js'

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
        ...pkg.peerDependencies && omit(Object.keys(existingDeps), pkg.peerDependencies),
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
  const specsDiff = diffFlatRecords(specs, existingDeps)
  if (!isEqual(specsDiff)) {
    return {
      satisfies: false,
      detailedReason: `specifiers in the lockfile don't match specifiers in package.json:\n${displaySpecDiff(specsDiff)}`,
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
      if (importer?.specifiers[depName] == null || !semver.validRange(importer?.specifiers[depName])) continue
      const version = dp.removeSuffix(importerDeps[depName])
      if (semver.valid(version) && !semver.satisfies(version, importer.specifiers[depName])) {
        return {
          satisfies: false,
          detailedReason: `The importer resolution is broken at dependency "${depName}": version "${version}" doesn't satisfy range "${importer.specifiers[depName]}"`,
        }
      }
    }
  }
  return { satisfies: true }
}

function countOfNonLinkedDeps (lockfileDeps: { [depName: string]: string }): number {
  return Object.values(lockfileDeps).filter((ref) => !ref.includes('link:') && !ref.includes('file:')).length
}

function displaySpecDiff ({ added, removed, modified }: Diff<string, string>): string {
  let result = ''

  if (added.length !== 0) {
    result += `* ${added.length} dependencies were added: `
    result += added.map(({ key, value }) => `${key}@${value}`).join(', ')
    result += '\n'
  }

  if (removed.length !== 0) {
    result += `* ${removed.length} dependencies were removed: `
    result += removed.map(({ key, value }) => `${key}@${value}`).join(', ')
    result += '\n'
  }

  if (modified.length !== 0) {
    result += `* ${modified.length} dependencies are mismatched:\n`
    for (const { key, left, right } of modified) {
      result += `  - ${key} (lockfile: ${left}, manifest: ${right})\n`
    }
  }

  return result
}
