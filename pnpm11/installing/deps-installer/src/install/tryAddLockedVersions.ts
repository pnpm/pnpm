import type { Catalogs } from '@pnpm/catalogs.types'
import type { VersionOverride } from '@pnpm/config.parse-overrides'
import type { LockfileObject } from '@pnpm/lockfile.types'
import {
  applyPackageSpecs,
  calcVersionRange,
  getAllDependenciesFromManifest,
  guessDependencyType,
  type PackageSpecObject,
} from '@pnpm/pkg-manifest.utils'
import { parseWantedDependency } from '@pnpm/resolving.parse-wanted-dependency'
import type { WorkspacePackages } from '@pnpm/resolving.resolver-base'
import type {
  DependenciesField,
  ProjectManifest,
  ProjectRootDir,
  RangeSpecStyle,
} from '@pnpm/types'
import { clone } from 'ramda'
import semver from 'semver'

import { lockedVersionResolutionWouldPick } from './tryFastUpdateImporters.js'
import { isDirectoryDependency } from './tryFastUpdateSettings.js'

export interface AddedDependencies {
  rootDir: ProjectRootDir
  manifest: ProjectManifest
  originalManifest?: ProjectManifest
  dependencySelectors: string[]
  targetDependenciesField?: DependenciesField
  rangeSpecStyle?: RangeSpecStyle
  allowNew?: boolean
  peer?: boolean
  update?: boolean
  updateToLatest?: boolean
  updatePackageManifest?: boolean
}

/** The manifests of one project with the requested dependencies saved. */
export interface AddedManifests {
  manifest: ProjectManifest
  originalManifest?: ProjectManifest
}

export interface AddLockedVersionsOptions {
  added: AddedDependencies[]
  autoInstallPeers: boolean
  catalogs?: Catalogs
  catalogMode: 'strict' | 'prefer' | 'manual'
  ignoreCurrentSpecifiers: boolean
  parsedOverrides: VersionOverride[]
  /**
   * Whether `resolutionMode` resolves a direct dependency to its lowest
   * satisfying version rather than its highest.
   */
  resolutionPicksLowest: boolean
  saveCatalogName?: string
  workspacePackages: WorkspacePackages
}

/**
 * The manifests `pnpm add` would write for every requested selector, on copies
 * the caller commits only once the lockfile rewrite they enable has passed its
 * gates — so a fallback to resolution still sees the `package.json` files as
 * they are on disk.
 *
 * `null` for anything the lockfile alone cannot answer, which leaves the caller
 * on the full-resolution path.
 */
export function tryAddLockedVersions (
  lockfile: LockfileObject,
  opts: AddLockedVersionsOptions
): Map<ProjectRootDir, AddedManifests> | null {
  // `catalogMode` and `saveCatalogName` rewrite the saved entry into a
  // `catalog:` reference, and `ignoreCurrentSpecifiers` drops the manifest
  // entry that decides the saved range's style.
  if (opts.catalogMode !== 'manual' || opts.saveCatalogName != null || opts.ignoreCurrentSpecifiers) {
    return null
  }
  const added = new Map<ProjectRootDir, AddedManifests>()
  for (const project of opts.added) {
    // Every project stages onto a fresh copy of its context manifest, so a
    // second mutation for the same one would discard the first one's edits.
    if (added.has(project.rootDir)) return null
    if (
      project.update === true ||
      project.updateToLatest === true ||
      project.peer === true ||
      project.allowNew === false ||
      project.updatePackageManifest === false
    ) {
      return null
    }
    const specs: PackageSpecObject[] = []
    for (const selector of project.dependencySelectors) {
      const spec = lockedAddSpec(lockfile, selector, { project, opts })
      if (spec == null) return null
      specs.push(spec)
    }
    added.set(project.rootDir, {
      manifest: applyPackageSpecs(clone(project.manifest), specs),
      originalManifest: project.originalManifest && applyPackageSpecs(clone(project.originalManifest), specs),
    })
  }
  return added
}

/** The manifest entry a single `pnpm add` selector saves, or `null`. */
function lockedAddSpec (
  lockfile: LockfileObject,
  selector: string,
  { project, opts }: { project: AddedDependencies, opts: AddLockedVersionsOptions }
): PackageSpecObject | null {
  const { alias, bareSpecifier } = parseWantedDependency(selector)
  // Without a version the request means the `latest` tag, the specifier a
  // sibling project prefers, or the one an override names — none of which the
  // lockfile decides.
  if (alias == null || bareSpecifier == null) return null
  if (semver.validRange(bareSpecifier) == null) return null
  if (isDirectoryDependency(alias, bareSpecifier, opts.workspacePackages)) return null
  // An override rewrites both what the request resolves to and the range saved
  // for it.
  if (opts.parsedOverrides.some(({ targetPkg }) => targetPkg.name === alias)) return null
  if (opts.catalogs?.default != null && Object.hasOwn(opts.catalogs.default, alias)) return null
  // A dependency only `peerDependencies` declares stays there, and the
  // manifest writer records nothing for it.
  if (project.targetDependenciesField == null && guessDependencyType(alias, project.manifest) === 'peerDependencies') {
    return null
  }
  // `Object.hasOwn` keeps a package legitimately named `constructor` or
  // `toString` from reading a specifier off `Object.prototype`.
  const manifestDependencies = getAllDependenciesFromManifest(project.manifest, {
    autoInstallPeers: opts.autoInstallPeers,
  })
  const prevSpecifier = Object.hasOwn(manifestDependencies, alias) ? manifestDependencies[alias] : undefined
  // The range style a re-add keeps is only readable off a registry-style
  // specifier.
  if (prevSpecifier != null && semver.validRange(prevSpecifier) == null) return null
  const version = lockedVersionResolutionWouldPick(lockfile, alias, {
    specifier: bareSpecifier,
    resolutionPicksLowest: opts.resolutionPicksLowest,
  })
  if (version == null) return null
  return {
    alias,
    bareSpecifier: calcVersionRange(version, {
      prevSpecifier,
      bareSpecifier,
      defaultRangeSpecStyle: project.rangeSpecStyle,
    }),
    resolvedVersion: version,
    rangeSpecStyle: project.rangeSpecStyle,
    saveType: project.targetDependenciesField,
  }
}
