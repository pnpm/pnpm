import logger from '@pnpm/logger'
import { getAllDependenciesFromManifest } from '@pnpm/manifest-utils'
import {
  PreferredVersions,
  WorkspacePackages,
} from '@pnpm/resolver-base'
import { Dependencies, ProjectManifest } from '@pnpm/types'
import getVerSelType from 'version-selector-type'
import getWantedDependencies from './getWantedDependencies'
import { WantedDependency } from './getNonDevWantedDependencies'
import { Importer, ImporterToResolve } from './resolveDependencyTree'
import safeIsInnerLink from './safeIsInnerLink'

export default async function toResolveImporter<T> (
  opts: {
    defaultUpdateDepth: number
    lockfileOnly: boolean
    preferredVersions?: PreferredVersions
    updateAll: boolean
    virtualStoreDir: string
    workspacePackages: WorkspacePackages
  },
  project: Importer<T>
): Promise<ImporterToResolve<T>> {
  // eslint-disable-next-line
  const allDeps = getWantedDependencies(project.manifest) as any
  const { nonLinkedDependencies } = await partitionLinkedPackages(allDeps, {
    lockfileOnly: opts.lockfileOnly,
    modulesDir: project.modulesDir,
    projectDir: project.rootDir,
    virtualStoreDir: opts.virtualStoreDir,
    workspacePackages: opts.workspacePackages,
  })
  const existingDeps = nonLinkedDependencies
    // eslint-disable-next-line
    .filter(({ alias }) => !project.wantedDependencies.some((wantedDep) => wantedDep.alias === alias)) as any
  let wantedDependencies!: Array<T & WantedDependency & { isNew?: boolean, updateDepth: number }>
  if (!project.manifest) {
    wantedDependencies = [
      ...project.wantedDependencies,
      ...existingDeps,
    ]
      .map((dep) => ({
        ...dep,
        updateDepth: opts.defaultUpdateDepth,
      })) as any // eslint-disable-line
  } else {
    // Direct local tarballs are always checked,
    // so their update depth should be at least 0
    const updateLocalTarballs = (dep: T & WantedDependency) => ({
      ...dep,
      updateDepth: opts.updateAll
        ? opts.defaultUpdateDepth
        : (prefIsLocalTarball(dep.pref) ? 0 : -1),
    })
    wantedDependencies = [
      ...project.wantedDependencies.map(
        opts.defaultUpdateDepth < 0
          ? updateLocalTarballs
          : (dep) => ({ ...dep, updateDepth: opts.defaultUpdateDepth })),
      ...existingDeps.map(updateLocalTarballs),
    ] as any // eslint-disable-line
  }
  return {
    ...project,
    hasRemovedDependencies: Boolean(project.removePackages?.length),
    preferredVersions: opts.preferredVersions ?? (project.manifest && getPreferredVersionsFromPackage(project.manifest)) ?? {},
    wantedDependencies,
  }
}

function prefIsLocalTarball (pref: string) {
  return pref.startsWith('file:') && pref.endsWith('.tgz')
}

async function partitionLinkedPackages<T> (
  dependencies: Array<T & WantedDependency>,
  opts: {
    projectDir: string
    lockfileOnly: boolean
    modulesDir: string
    virtualStoreDir: string
    workspacePackages?: WorkspacePackages
  }
): Promise<{
    linkedAliases: Set<string>
    nonLinkedDependencies: Array<WantedDependency & T>
  }> {
  const nonLinkedDependencies: Array<WantedDependency & T> = []
  const linkedAliases = new Set<string>()
  for (const dependency of dependencies) {
    if (
      !dependency.alias ||
      opts.workspacePackages?.[dependency.alias] != null ||
      dependency.pref.startsWith('workspace:')
    ) {
      nonLinkedDependencies.push(dependency)
      continue
    }
    const isInnerLink = await safeIsInnerLink(opts.modulesDir, dependency.alias, {
      hideAlienModules: !opts.lockfileOnly,
      projectDir: opts.projectDir,
      virtualStoreDir: opts.virtualStoreDir,
    })
    if (isInnerLink === true) {
      nonLinkedDependencies.push(dependency)
      continue
    }
    // This info-log might be better to be moved to the reporter
    logger.info({
      message: `${dependency.alias} is linked to ${opts.modulesDir} from ${isInnerLink}`,
      prefix: opts.projectDir,
    })
    linkedAliases.add(dependency.alias)
  }
  return {
    linkedAliases,
    nonLinkedDependencies,
  }
}

function getPreferredVersionsFromPackage (
  pkg: Pick<ProjectManifest, 'devDependencies' | 'dependencies' | 'optionalDependencies'>
): PreferredVersions {
  return getVersionSpecsByRealNames(getAllDependenciesFromManifest(pkg))
}

function getVersionSpecsByRealNames (deps: Dependencies) {
  return Object.keys(deps)
    .reduce((acc, depName) => {
      if (deps[depName].startsWith('npm:')) {
        const pref = deps[depName].substr(4)
        const index = pref.lastIndexOf('@')
        const spec = pref.substr(index + 1)
        const selector = getVerSelType(spec)
        if (selector != null) {
          const pkgName = pref.substr(0, index)
          acc[pkgName] = acc[pkgName] || {}
          acc[pkgName][selector.normalized] = selector.type
        }
      } else if (!deps[depName].includes(':')) { // we really care only about semver specs
        const selector = getVerSelType(deps[depName])
        if (selector != null) {
          acc[depName] = acc[depName] || {}
          acc[depName][selector.normalized] = selector.type
        }
      }
      return acc
    }, {})
}
