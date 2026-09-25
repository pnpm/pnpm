import { createMatcher } from '@pnpm/config.matcher'
import * as dp from '@pnpm/deps.path'
import type { ProjectSnapshot } from '@pnpm/lockfile.types'
import {
  DEPENDENCIES_FIELDS,
  type ProjectManifest,
} from '@pnpm/types'
import { equals, omit, pickBy } from 'ramda'
import semver from 'semver'

import { type Diff, diffFlatRecords, isEqual } from './diffFlatRecords.js'
import { dependencySpecifiersAreEqual } from './gitSpecifiersAreEquivalent.js'
import { unresolvedOptionalDependencies } from './unresolvedOptionalDependencies.js'

export function satisfiesPackageManifest (
  opts: {
    autoInstallPeers?: boolean
    excludeLinksFromLockfile?: boolean
    ignoredOptionalDependencies?: string[]
    /**
     * Treat an `optionalDependencies` entry the importer has no entry for as
     * satisfied: the install that wrote the lockfile could not resolve it
     * and skipped it. Only a frozen install may assume this; a resolving
     * install retries the dependency instead.
     */
    allowUnresolvedOptionalDependencies?: boolean
  },
  importer: ProjectSnapshot | undefined,
  pkg: ProjectManifest
): { satisfies: boolean, detailedReason?: string } {
  if (!importer) return { satisfies: false, detailedReason: 'no importer' }
  const ignoredOptionalDependencies = new Set(
    opts.ignoredOptionalDependencies?.length
      ? Object.keys(pkg.optionalDependencies ?? {})
        .filter(createMatcher(opts.ignoredOptionalDependencies))
      : []
  )
  if (opts.allowUnresolvedOptionalDependencies) {
    for (const depName of Object.keys(unresolvedOptionalDependencies(opts, importer, pkg))) {
      ignoredOptionalDependencies.add(depName)
    }
  }
  let existingDeps = omitIgnoredDependencies(
    { ...pkg.devDependencies, ...pkg.dependencies, ...pkg.optionalDependencies },
    ignoredOptionalDependencies
  )
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
  const specsDiff = diffFlatRecords(specs, existingDeps, dependencySpecifiersAreEqual)
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
  const lockfileLinksPublishDirectory = importer.publishDirectory != null && importer.linkDirectory !== false
  const manifestLinksPublishDirectory = pkg.publishConfig?.directory != null && pkg.publishConfig.linkDirectory !== false
  if (lockfileLinksPublishDirectory !== manifestLinksPublishDirectory) {
    return {
      satisfies: false,
      detailedReason: `"linkDirectory" in the lockfile (${lockfileLinksPublishDirectory}) doesn't match "publishConfig.linkDirectory" in package.json (${manifestLinksPublishDirectory})`,
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
    let pkgDeps = depField === 'devDependencies'
      ? pkg[depField] ?? {}
      : omitIgnoredDependencies(pkg[depField], ignoredOptionalDependencies)
    if (opts?.excludeLinksFromLockfile) {
      pkgDeps = pickNonLinkedDeps(pkgDeps)
    }

    let requiredDepNames: string[]
    let allowedDepNames: Set<string>
    switch (depField) {
      case 'optionalDependencies':
        requiredDepNames = Object.keys(pkgDeps)
        allowedDepNames = new Set(requiredDepNames)
        break
      case 'devDependencies':
        requiredDepNames = Object.keys(pkgDeps)
          .filter((depName) => !pkg.optionalDependencies?.[depName] && !pkg.dependencies?.[depName])
        allowedDepNames = new Set(
          Object.keys(pkgDeps).filter((depName) => !pkg.optionalDependencies?.[depName])
        )
        break
      case 'dependencies':
        requiredDepNames = Object.keys(pkgDeps)
          .filter((depName) => !pkg.optionalDependencies?.[depName])
        allowedDepNames = new Set(requiredDepNames)
        break
      default:
        throw new Error(`Unknown dependency type "${depField as string}"`)
    }
    for (const depName of requiredDepNames) {
      if (!importerDeps[depName]) {
        return {
          satisfies: false,
          detailedReason: `"${depField}" in the lockfile (${JSON.stringify(importerDeps)}) doesn't match the same field in package.json (${JSON.stringify(pkgDeps)})`,
        }
      }
    }
    for (const [depName, ref] of Object.entries(importerDeps)) {
      if (ref.includes('link:') || ref.includes('file:')) continue
      if (!allowedDepNames.has(depName)) {
        return {
          satisfies: false,
          detailedReason: `"${depField}" in the lockfile (${JSON.stringify(importerDeps)}) doesn't match the same field in package.json (${JSON.stringify(pkgDeps)})`,
        }
      }
      const manifestSpecifier = depField === 'devDependencies' && pkg.dependencies?.[depName] != null
        ? pkg.dependencies[depName]
        : pkgDeps[depName]
      if (!dependencySpecifiersAreEqual(importer.specifiers?.[depName], manifestSpecifier)) {
        return {
          satisfies: false,
          detailedReason: `importer ${depField}.${depName} specifier ${importer.specifiers[depName]} don't match package manifest specifier (${manifestSpecifier})`,
        }
      }
      if (importer?.specifiers[depName] == null || !semver.validRange(importer?.specifiers[depName])) continue
      const version = dp.removeSuffix(ref)
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

function omitIgnoredDependencies (
  dependencies: Record<string, string> | undefined,
  ignoredDependencies: Set<string>
): Record<string, string> {
  const filteredDependencies = { ...dependencies }
  for (const dependency of ignoredDependencies) {
    delete filteredDependencies[dependency]
  }
  return filteredDependencies
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
