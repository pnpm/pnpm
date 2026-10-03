import { logger } from '@pnpm/logger'
import { getAllDependenciesFromManifest } from '@pnpm/pkg-manifest.utils'
import type {
  PreferredVersions,
  VersionSelectors,
  WorkspacePackages,
} from '@pnpm/resolving.resolver-base'
import type { Dependencies, ProjectManifest } from '@pnpm/types'
import getVerSelType from 'version-selector-type'

import { getWantedDependencies, type ManifestWantedDependency, type WantedDependency } from './getWantedDependencies.js'
import type { ImporterToResolve } from './index.js'
import type { ImporterToResolveGeneric } from './resolveDependencyTree.js'
import { safeIsInnerLink } from './safeIsInnerLink.js'
import { unwrapPackageName } from './unwrapPackageName.js'
import { validatePeerDependencies } from './validatePeerDependencies.js'

export interface ResolveImporter extends ImporterToResolve, ImporterToResolveGeneric<object> {
  wantedDependencies: Array<WantedDependency & {
    updateDepth: number
  }>
}

type WantedDependencyWithUpdateDepth = WantedDependency & { updateDepth: number }

interface ToResolveImporterOptions {
  autoInstallPeers?: boolean
  defaultUpdateDepth: number
  hideAlienModules: boolean
  preferredVersions?: PreferredVersions
  preferredVersionsByImporterId?: Record<string, PreferredVersions>
  virtualStoreDir: string
  globalVirtualStoreDir: string
  workspacePackages: WorkspacePackages
  updateToLatest?: boolean
  noDependencySelectors: boolean
}

export async function toResolveImporter (
  opts: ToResolveImporterOptions,
  project: ImporterToResolve
): Promise<ResolveImporter> {
  validatePeerDependencies(project)
  const allDeps = getWantedDependencies(project.manifest, { autoInstallPeers: opts.autoInstallPeers })
  const nonLinkedDependencies = await partitionLinkedPackages(allDeps, {
    hideAlienModules: opts.hideAlienModules,
    modulesDir: project.modulesDir,
    projectDir: project.rootDir,
    virtualStoreDir: opts.virtualStoreDir,
    globalVirtualStoreDir: opts.globalVirtualStoreDir,
    workspacePackages: opts.workspacePackages,
  })
  const defaultUpdateDepth = (project.update === true || (project.updateMatching != null)) ? opts.defaultUpdateDepth : -1
  const existingDeps = getExistingDependencies(project, nonLinkedDependencies)
  if (opts.updateToLatest && opts.noDependencySelectors) {
    for (const dep of existingDeps) {
      dep.updateSpec = true
    }
  }
  const wantedDependencies = assignUpdateDepths(project, { defaultUpdateDepth, existingDeps })
  return {
    ...project,
    hasRemovedDependencies: Boolean(project.removePackages?.length),
    preferredVersions: getProjectPreferredVersions(opts, project),
    wantedDependencies,
  }
}

function getExistingDependencies (
  project: ImporterToResolve,
  nonLinkedDependencies: ManifestWantedDependency[]
): ManifestWantedDependency[] {
  return nonLinkedDependencies
    .filter(({ alias }) => !project.wantedDependencies.some((wantedDep) => wantedDep.alias === alias))
    .map((dependency) => project.hookOwnedAliases?.has(dependency.alias)
      ? {
        ...dependency,
        saveSpec: false,
        updateToLatestAllowed: false,
      }
      : dependency)
}

function assignUpdateDepths (
  project: ImporterToResolve,
  opts: {
    defaultUpdateDepth: number
    existingDeps: WantedDependency[]
  }
): WantedDependencyWithUpdateDepth[] {
  const { defaultUpdateDepth, existingDeps } = opts
  if (!project.manifest) {
    return [
      ...project.wantedDependencies,
      ...existingDeps,
    ]
      .map((dep) => ({
        ...dep,
        updateDepth: defaultUpdateDepth,
      }))
  }
  // Direct local tarballs are always checked,
  // so their update depth should be at least 0
  const updateLocalTarballs = (dep: WantedDependency) => ({
    ...dep,
    updateDepth: project.updateMatching != null
      ? defaultUpdateDepth
      : (prefIsLocalTarball(dep.bareSpecifier) ? 0 : defaultUpdateDepth),
  })
  return [
    ...project.wantedDependencies.map(
      defaultUpdateDepth < 0
        ? updateLocalTarballs
        : (dep) => ({ ...dep, updateDepth: defaultUpdateDepth })),
    ...existingDeps.map(
      project.updateMatching != null
        ? updateLocalTarballs
        : (dep) => ({ ...dep, updateDepth: -1 })
    ),
  ]
}

function getProjectPreferredVersions (
  opts: Pick<ToResolveImporterOptions, 'preferredVersions' | 'preferredVersionsByImporterId'>,
  project: ImporterToResolve
): PreferredVersions {
  const sharedPreferredVersions = opts.preferredVersions ?? (project.manifest && getPreferredVersionsFromPackage(project.manifest)) ?? {}
  const projectPins = opts.preferredVersionsByImporterId?.[project.id]
  return projectPins == null
    ? sharedPreferredVersions
    : overlayProjectVersionPins(sharedPreferredVersions, projectPins)
}

const LOCAL_TARBALL_PATTERN = /\.(?:tgz|tar\.gz|tar|tar\.bz2|tbz2|tbz)$/i

function prefIsLocalTarball (bareSpecifier: string): boolean {
  return bareSpecifier.startsWith('file:') && LOCAL_TARBALL_PATTERN.test(bareSpecifier)
}

async function partitionLinkedPackages<Dependency extends WantedDependency> (
  dependencies: Dependency[],
  opts: {
    projectDir: string
    hideAlienModules: boolean
    modulesDir: string
    virtualStoreDir: string
    globalVirtualStoreDir: string
    workspacePackages?: WorkspacePackages
  }
): Promise<Dependency[]> {
  const nonLinkedDependencies: Dependency[] = []
  await Promise.all(dependencies.map(async (dependency) => {
    if (
      !dependency.alias ||
      opts.workspacePackages?.get(dependency.alias) != null ||
      dependency.bareSpecifier.startsWith('workspace:')
    ) {
      nonLinkedDependencies.push(dependency)
      return
    }
    const isInnerLink = await safeIsInnerLink(opts.modulesDir, dependency.alias, {
      hideAlienModules: opts.hideAlienModules,
      projectDir: opts.projectDir,
      virtualStoreDir: opts.virtualStoreDir,
      globalVirtualStoreDir: opts.globalVirtualStoreDir,
    })
    if (isInnerLink === true) {
      nonLinkedDependencies.push(dependency)
      return
    }
    if (!dependency.bareSpecifier.startsWith('link:')) {
      // This info-log might be better to be moved to the reporter
      logger.info({
        message: `${dependency.alias} is linked to ${opts.modulesDir} from ${isInnerLink}`,
        prefix: opts.projectDir,
      })
    }
  }))
  return nonLinkedDependencies
}

// A project's own pins replace shared concrete versions for names that the
// project's lockfile records, so an older pin stays selected when a shared
// pin also satisfies the range.
function overlayProjectVersionPins (
  shared: PreferredVersions,
  projectPins: PreferredVersions
): PreferredVersions {
  const preferredVersions: PreferredVersions = Object.assign(Object.create(null), shared)
  for (const [name, pins] of Object.entries(projectPins)) {
    const selectors: VersionSelectors = Object.assign(Object.create(null), preferredVersions[name])
    for (const [selector, info] of Object.entries(selectors)) {
      const selectorType = typeof info === 'string' ? info : info.selectorType
      if (selectorType === 'version') {
        delete selectors[selector]
      }
    }
    Object.assign(selectors, pins)
    preferredVersions[name] = selectors
  }
  return preferredVersions
}

function getPreferredVersionsFromPackage (
  pkg: Pick<ProjectManifest, 'devDependencies' | 'dependencies' | 'optionalDependencies'>
): PreferredVersions {
  return getVersionSpecsByRealNames(getAllDependenciesFromManifest(pkg))
}

type VersionSpecsByRealNames = Record<string, Record<string, 'version' | 'range' | 'tag'>>

function getVersionSpecsByRealNames (deps: Dependencies): VersionSpecsByRealNames {
  const acc: VersionSpecsByRealNames = {}
  for (const depName in deps) {
    const currentBareSpecifier = deps[depName]

    const { pkgName, bareSpecifier } = unwrapPackageName(depName, currentBareSpecifier)

    // we really care only about semver specs
    if (bareSpecifier.includes(':')) {
      continue
    }

    const selector = getVerSelType(bareSpecifier)
    if (selector != null) {
      if (!Object.hasOwn(acc, pkgName)) {
        acc[pkgName] = {}
      }
      acc[pkgName][selector.normalized] = selector.type
    }
  }
  return acc
}
