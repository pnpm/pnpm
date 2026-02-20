import path from 'path'
import {
  readProjectManifestOnly,
  tryReadProjectManifest,
} from '@pnpm/cli-utils'
import { type Config, getOptionsFromRootManifest } from '@pnpm/config'
import { checkDepsStatus } from '@pnpm/deps.status'
import { PnpmError } from '@pnpm/error'
import { arrayOfWorkspacePackagesToMap } from '@pnpm/get-context'
import { filterPkgsBySelectorObjects } from '@pnpm/filter-workspace-packages'
import { filterDependenciesByType } from '@pnpm/manifest-utils'
import { findWorkspacePackages } from '@pnpm/workspace.find-packages'
import { type LockfileObject } from '@pnpm/lockfile.types'
import { rebuildProjects } from '@pnpm/plugin-commands-rebuild'
import { createStoreController, type CreateStoreControllerOptions } from '@pnpm/store-connection-manager'
import {
  type IncludedDependencies,
  type Project,
  type ProjectsGraph,
  type ProjectRootDir,
  type PackageVulnerabilityAudit,
  type VulnerabilitySeverity,
} from '@pnpm/types'
import {
  IgnoredBuildsError,
  install,
  mutateModulesInSingleProject,
  type MutateModulesOptions,
  type UpdateMatchingFunction,
  type WorkspacePackages,
} from '@pnpm/core'
import { globalInfo, logger } from '@pnpm/logger'
import { sequenceGraph } from '@pnpm/sort-packages'
import { updateWorkspaceManifest } from '@pnpm/workspace.manifest-writer'
import { createPkgGraph } from '@pnpm/workspace.pkgs-graph'
import { updateWorkspaceState, type WorkspaceStateSettings } from '@pnpm/workspace.state'
import { type PreferredVersions, type VersionSelectors } from '@pnpm/resolver-base'
import { getPinnedVersion } from './getPinnedVersion.js'
import { getSaveType } from './getSaveType.js'
import {
  type CommandFullName,
  type RecursiveOptions,
  type UpdateDepsMatcher,
  createMatcher,
  matchDependencies,
  makeIgnorePatterns,
  recursive,
} from './recursive.js'
import { createWorkspaceSpecs, updateToWorkspacePackagesFromManifest } from './updateWorkspaceDependencies.js'

const OVERWRITE_UPDATE_OPTIONS = {
  allowNew: true,
  update: false,
}

export type InstallDepsOptions = Pick<Config,
| 'allProjects'
| 'allProjectsGraph'
| 'autoInstallPeers'
| 'bail'
| 'bin'
| 'catalogs'
| 'catalogMode'
| 'cleanupUnusedCatalogs'
| 'cliOptions'
| 'dedupePeerDependents'
| 'depth'
| 'dev'
| 'enableGlobalVirtualStore'
| 'engineStrict'
| 'excludeLinksFromLockfile'
| 'global'
| 'globalPnpmfile'
| 'hooks'
| 'ignoreCurrentSpecifiers'
| 'ignorePnpmfile'
| 'ignoreScripts'
| 'optimisticRepeatInstall'
| 'linkWorkspacePackages'
| 'lockfileDir'
| 'lockfileOnly'
| 'production'
| 'preferWorkspacePackages'
| 'rawLocalConfig'
| 'registries'
| 'rootProjectManifestDir'
| 'rootProjectManifest'
| 'save'
| 'saveDev'
| 'saveExact'
| 'saveOptional'
| 'savePeer'
| 'savePrefix'
| 'saveProd'
| 'saveWorkspaceProtocol'
| 'lockfileIncludeTarballUrl'
| 'scriptsPrependNodePath'
| 'scriptShell'
| 'selectedProjectsGraph'
| 'sideEffectsCache'
| 'sideEffectsCacheReadonly'
| 'sort'
| 'sharedWorkspaceLockfile'
| 'shellEmulator'
| 'tag'
| 'allowBuilds'
| 'optional'
| 'workspaceConcurrency'
| 'workspaceDir'
| 'workspacePackagePatterns'
| 'extraEnv'
| 'ignoreWorkspaceCycles'
| 'disallowWorkspaceCycles'
| 'configDependencies'
| 'updateConfig'
> & CreateStoreControllerOptions & {
  argv: {
    original: string[]
  }
  allowNew?: boolean
  forceFullResolution?: boolean
  frozenLockfileIfExists?: boolean
  include?: IncludedDependencies
  includeDirect?: IncludedDependencies
  latest?: boolean
  /**
   * If specified, the installation will only be performed for comparison of the
   * wanted lockfile. The wanted lockfile will not be updated on disk and no
   * modules will be linked.
   *
   * The given callback is passed the wanted lockfile before installation and
   * after. This allows functions to reasonably determine whether the wanted
   * lockfile will change on disk after installation. The lockfile arguments
   * passed to this callback should not be mutated.
   */
  lockfileCheck?: (prev: LockfileObject, next: LockfileObject) => void
  update?: boolean
  updateToLatest?: boolean
  updateMatching?: UpdateMatchingFunction
  updatePackageManifest?: boolean
  useBetaCli?: boolean
  recursive?: boolean
  dedupe?: boolean
  workspace?: boolean
  includeOnlyPackageFiles?: boolean
  fetchFullMetadata?: boolean
  pruneLockfileImporters?: boolean
  pnpmfile: string[]
  packageVulnerabilityAudit?: PackageVulnerabilityAudit
} & Partial<Pick<Config, 'pnpmHomeDir' | 'strictDepBuilds'>>

export async function installDeps (
  opts: InstallDepsOptions,
  params: string[]
): Promise<void> {
  if (!opts.update && !opts.dedupe && params.length === 0 && opts.optimisticRepeatInstall) {
    const { upToDate } = await checkDepsStatus({
      ...opts,
      ignoreFilteredInstallCache: true,
    })
    if (upToDate) {
      globalInfo('Already up to date')
      return
    }
  }
  if (opts.workspace) {
    if (opts.latest) {
      throw new PnpmError('BAD_OPTIONS', 'Cannot use --latest with --workspace simultaneously')
    }
    if (!opts.workspaceDir) {
      throw new PnpmError('WORKSPACE_OPTION_OUTSIDE_WORKSPACE', '--workspace can only be used inside a workspace')
    }
    if (!opts.linkWorkspacePackages && !opts.saveWorkspaceProtocol) {
      if (opts.rawLocalConfig['save-workspace-protocol'] === false) {
        throw new PnpmError('BAD_OPTIONS', 'This workspace has link-workspace-packages turned off, \
so dependencies are linked from the workspace only when the workspace protocol is used. \
Either set link-workspace-packages to true or don\'t use the --no-save-workspace-protocol option \
when running add/update with the --workspace option')
      } else {
        opts.saveWorkspaceProtocol = true
      }
    }
    // @ts-expect-error
    opts['preserveWorkspaceProtocol'] = !opts.linkWorkspacePackages
  }
  const store = await createStoreController(opts)
  const includeDirect = opts.includeDirect ?? {
    dependencies: true,
    devDependencies: true,
    optionalDependencies: true,
  }
  const forceHoistPattern = typeof opts.rawLocalConfig['hoist-pattern'] !== 'undefined' ||
    typeof opts.rawLocalConfig['hoist'] !== 'undefined'
  const forcePublicHoistPattern = typeof opts.rawLocalConfig['shamefully-hoist'] !== 'undefined' ||
    typeof opts.rawLocalConfig['public-hoist-pattern'] !== 'undefined'
  const allProjects = opts.allProjects ?? (
    opts.workspaceDir
      ? await findWorkspacePackages(opts.workspaceDir, { ...opts, patterns: opts.workspacePackagePatterns })
      : []
  )
  if (opts.workspaceDir) {
    const selectedProjectsGraph = opts.selectedProjectsGraph ?? selectProjectByDir(allProjects, opts.dir)
    if (selectedProjectsGraph != null) {
      const sequencedGraph = sequenceGraph(selectedProjectsGraph)
      // Check and warn if there are cyclic dependencies
      if (!opts.ignoreWorkspaceCycles && !sequencedGraph.safe) {
        const cyclicDependenciesInfo = sequencedGraph.cycles.length > 0
          ? `: ${sequencedGraph.cycles.map(deps => deps.join(', ')).join('; ')}`
          : ''

        if (opts.disallowWorkspaceCycles) {
          throw new PnpmError('DISALLOW_WORKSPACE_CYCLES', `There are cyclic workspace dependencies${cyclicDependenciesInfo}`)
        }

        logger.warn({
          message: `There are cyclic workspace dependencies${cyclicDependenciesInfo}`,
          prefix: opts.workspaceDir,
        })
      }

      const allProjectsGraph: ProjectsGraph = opts.allProjectsGraph ?? createPkgGraph(allProjects, {
        linkWorkspacePackages: Boolean(opts.linkWorkspacePackages),
      }).graph

      const recursiveRootManifestOpts = getOptionsFromRootManifest(opts.rootProjectManifestDir, opts.rootProjectManifest ?? {})
      await recursiveInstallThenUpdateWorkspaceState(allProjects,
        params,
        {
          ...opts,
          ...recursiveRootManifestOpts,
          allowBuilds: {
            ...recursiveRootManifestOpts.allowBuilds,
            ...opts.allowBuilds,
          },
          forceHoistPattern,
          forcePublicHoistPattern,
          preferredVersions: opts.packageVulnerabilityAudit ? preferNonvulnerablePackageVersions(opts.packageVulnerabilityAudit) : undefined,
          allProjectsGraph,
          selectedProjectsGraph,
          storeControllerAndDir: store,
          workspaceDir: opts.workspaceDir,
        },
        opts.update ? 'update' : (params.length === 0 ? 'install' : 'add')
      )
      return
    }
  }
  // `pnpm install ""` is going to be just `pnpm install`
  params = params.filter(Boolean)

  const dir = opts.dir || process.cwd()
  let workspacePackages!: WorkspacePackages

  if (opts.workspaceDir) {
    workspacePackages = arrayOfWorkspacePackagesToMap(allProjects) as WorkspacePackages
  }

  let { manifest, writeProjectManifest } = await tryReadProjectManifest(opts.dir, opts)
  if (manifest === null) {
    if (opts.update === true || params.length === 0) {
      throw new PnpmError('NO_PKG_MANIFEST', `No package.json found in ${opts.dir}`)
    }
    manifest = {}
  }

  const rootManifestOpts = getOptionsFromRootManifest(opts.dir, (opts.dir === opts.rootProjectManifestDir ? opts.rootProjectManifest ?? manifest : manifest))
  const installOpts: Omit<MutateModulesOptions, 'allProjects'> = {
    ...opts,
    ...rootManifestOpts,
    allowBuilds: {
      ...rootManifestOpts.allowBuilds,
      ...opts.allowBuilds,
    },
    forceHoistPattern,
    forcePublicHoistPattern,
    // In case installation is done in a multi-package repository
    // The dependencies should be built first,
    // so ignoring scripts for now
    ignoreScripts: !!workspacePackages || opts.ignoreScripts,
    linkWorkspacePackagesDepth: opts.linkWorkspacePackages === 'deep' ? Infinity : opts.linkWorkspacePackages ? 0 : -1,
    sideEffectsCacheRead: opts.sideEffectsCache ?? opts.sideEffectsCacheReadonly,
    sideEffectsCacheWrite: opts.sideEffectsCache,
    storeController: store.ctrl,
    storeDir: store.dir,
    workspacePackages,
    preferredVersions: opts.packageVulnerabilityAudit ? preferNonvulnerablePackageVersions(opts.packageVulnerabilityAudit) : undefined,
  }

  let updateMatch: UpdateDepsMatcher | null
  let updatePackageManifest = opts.updatePackageManifest
  let updateMatching: UpdateMatchingFunction | undefined
  if (opts.update) {
    if (params.length === 0) {
      const ignoreDeps = opts.updateConfig?.ignoreDependencies
      if (ignoreDeps?.length) {
        params = makeIgnorePatterns(ignoreDeps)
      }
    }
    updateMatch = params.length ? createMatcher(params) : null
  } else {
    updateMatch = null
  }
  if (opts.packageVulnerabilityAudit != null) {
    updateMatch = null
    const { packageVulnerabilityAudit } = opts
    updateMatching = (pkgName: string, version?: string) => version != null && packageVulnerabilityAudit.isVulnerable(pkgName, version)
  }
  if (updateMatch != null) {
    params = matchDependencies(updateMatch, manifest, includeDirect)
    if (params.length === 0) {
      if (opts.latest) return
      if (opts.depth === 0) {
        throw new PnpmError('NO_PACKAGE_IN_DEPENDENCIES',
          'None of the specified packages were found in the dependencies.')
      }
      // No direct dependencies matched, so we're updating indirect dependencies only
      // Don't update package.json in this case, and limit updates to only matching dependencies
      updatePackageManifest = false
      updateMatching = (pkgName: string) => updateMatch!(pkgName) != null
    }
  }

  if (opts.update && opts.latest && (!params || (params.length === 0))) {
    params = Object.keys(filterDependenciesByType(manifest, includeDirect))
  }
  if (opts.workspace) {
    if (!params || (params.length === 0)) {
      params = updateToWorkspacePackagesFromManifest(manifest, includeDirect, workspacePackages)
    } else {
      params = createWorkspaceSpecs(params, workspacePackages)
    }
  }
  if (params?.length) {
    const mutatedProject = {
      allowNew: opts.allowNew,
      binsDir: opts.bin,
      dependencySelectors: params,
      manifest,
      mutation: 'installSome' as const,
      peer: opts.savePeer,
      pinnedVersion: getPinnedVersion(opts),
      rootDir: opts.dir as ProjectRootDir,
      targetDependenciesField: getSaveType(opts),
    }
    const { updatedCatalogs, updatedProject, ignoredBuilds } = await mutateModulesInSingleProject(mutatedProject, installOpts)
    if (opts.save !== false) {
      await Promise.all([
        writeProjectManifest(updatedProject.manifest),
        updateWorkspaceManifest(opts.workspaceDir ?? opts.dir, {
          updatedCatalogs,
          cleanupUnusedCatalogs: opts.cleanupUnusedCatalogs,
          allProjects: opts.allProjects,
        }),
      ])
    }
    if (!opts.lockfileOnly) {
      await updateWorkspaceState({
        allProjects,
        settings: opts,
        workspaceDir: opts.workspaceDir ?? opts.lockfileDir ?? opts.dir,
        pnpmfiles: opts.pnpmfile,
        filteredInstall: allProjects.length !== Object.keys(opts.selectedProjectsGraph ?? {}).length,
        configDependencies: opts.configDependencies,
      })
    }
    if (opts.strictDepBuilds && ignoredBuilds?.size) {
      throw new IgnoredBuildsError(ignoredBuilds)
    }
    return
  }

  const { updatedCatalogs, updatedManifest, ignoredBuilds } = await install(manifest, {
    ...installOpts,
    updatePackageManifest,
    updateMatching,
  })
  if (opts.update === true && opts.save !== false) {
    await Promise.all([
      writeProjectManifest(updatedManifest),
      updateWorkspaceManifest(opts.workspaceDir ?? opts.dir, {
        updatedCatalogs,
        cleanupUnusedCatalogs: opts.cleanupUnusedCatalogs,
        allProjects,
      }),
    ])
  }
  if (opts.strictDepBuilds && ignoredBuilds?.size) {
    throw new IgnoredBuildsError(ignoredBuilds)
  }

  if (opts.linkWorkspacePackages && opts.workspaceDir) {
    const { selectedProjectsGraph } = await filterPkgsBySelectorObjects(allProjects, [
      {
        excludeSelf: true,
        includeDependencies: true,
        parentDir: dir,
      },
    ], {
      workspaceDir: opts.workspaceDir,
    })
    await recursiveInstallThenUpdateWorkspaceState(allProjects, [], {
      ...opts,
      ...OVERWRITE_UPDATE_OPTIONS,
      allProjectsGraph: opts.allProjectsGraph!,
      selectedProjectsGraph,
      workspaceDir: opts.workspaceDir, // Otherwise TypeScript doesn't understand that is not undefined
    }, 'install')

    if (opts.ignoreScripts) return

    await rebuildProjects(
      [
        {
          buildIndex: 0,
          manifest: await readProjectManifestOnly(opts.dir, opts),
          rootDir: opts.dir as ProjectRootDir,
        },
      ], {
        ...opts,
        pending: true,
        storeController: store.ctrl,
        storeDir: store.dir,
        skipIfHasSideEffectsCache: true,
      }
    )
  } else {
    if (!opts.lockfileOnly) {
      await updateWorkspaceState({
        allProjects,
        settings: opts,
        workspaceDir: opts.workspaceDir ?? opts.lockfileDir ?? opts.dir,
        pnpmfiles: opts.pnpmfile,
        filteredInstall: allProjects.length !== Object.keys(opts.selectedProjectsGraph ?? {}).length,
        configDependencies: opts.configDependencies,
      })
    }
  }
}

function selectProjectByDir (projects: Project[], searchedDir: string): ProjectsGraph | undefined {
  const project = projects.find(({ rootDir }) => path.relative(rootDir, searchedDir) === '')
  if (project == null) return undefined
  return { [searchedDir]: { dependencies: [], package: project } }
}

async function recursiveInstallThenUpdateWorkspaceState (
  allProjects: Project[],
  params: string[],
  opts: RecursiveOptions & WorkspaceStateSettings,
  cmdFullName: CommandFullName
): Promise<boolean | string> {
  const recursiveResult = await recursive(allProjects, params, opts, cmdFullName)
  if (!opts.lockfileOnly) {
    await updateWorkspaceState({
      allProjects,
      settings: opts,
      workspaceDir: opts.workspaceDir,
      pnpmfiles: opts.pnpmfile,
      filteredInstall: allProjects.length !== Object.keys(opts.selectedProjectsGraph ?? {}).length,
      configDependencies: opts.configDependencies,
    })
  }
  return recursiveResult
}

function severityStringToNumber (severity: VulnerabilitySeverity): number {
  switch (severity) {
  case 'low': return 0
  case 'moderate': return 1
  case 'high': return 2
  case 'critical': return 3
  default: return -1
  }
}

function getVulnerabilityPenalty (severity: VulnerabilitySeverity): number {
  switch (severity) {
  case 'low': return -1100 // 100 more than DIRECT_DEP_SELECTOR_WEIGHT from @pnpm/resolver-base
  case 'moderate': return -2000
  case 'high': return -3000
  case 'critical': return -4000
  // Treat unrecognized severity as the lowest severity
  default: return -1100
  }
}

function preferNonvulnerablePackageVersions (packageVulnerabilityAudit: PackageVulnerabilityAudit): PreferredVersions {
  const preferredVersions: PreferredVersions = {}
  for (const [packageName, vulnerabilities] of packageVulnerabilityAudit.getVulnerabilities()) {
    const vulnerableRanges = new Map<string, VulnerabilitySeverity>()
    for (const vuln of vulnerabilities) {
      const existingSeverity = vulnerableRanges.get(vuln.versionRange)
      if (existingSeverity == null) {
        vulnerableRanges.set(vuln.versionRange, vuln.severity)
        continue
      }
      // Choose the highest severity for the same version range
      if (severityStringToNumber(vuln.severity) > severityStringToNumber(existingSeverity)) {
        vulnerableRanges.set(vuln.versionRange, vuln.severity)
      }
    }
    const preferredVersionSelectors: VersionSelectors = {}
    for (const [vulnRange, severity] of vulnerableRanges) {
      if (vulnRange === '__proto__' || vulnRange === 'constructor' || vulnRange === 'prototype') {
        // Prevent prototype pollution
        continue
      }
      preferredVersionSelectors[vulnRange] = {
        selectorType: 'range',
        weight: getVulnerabilityPenalty(severity),
      }
    }
    preferredVersions[packageName] = preferredVersionSelectors
  }
  return preferredVersions
}
