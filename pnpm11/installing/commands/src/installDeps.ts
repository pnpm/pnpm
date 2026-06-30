import path from 'node:path'

import { buildProjects } from '@pnpm/building.after-install'
import { mergeCatalogs } from '@pnpm/catalogs.config'
import type { Catalogs } from '@pnpm/catalogs.types'
import type { CommandHandler } from '@pnpm/cli.command'
import {
  readProjectManifestOnly,
  tryReadProjectManifest,
} from '@pnpm/cli.utils'
import type { Config, ConfigContext } from '@pnpm/config.reader'
import { checkDepsStatus } from '@pnpm/deps.status'
import { PnpmError } from '@pnpm/error'
import { arrayOfWorkspacePackagesToMap } from '@pnpm/installing.context'
import {
  type DryRunInstallResult,
  install,
  mutateModulesInSingleProject,
  type MutateModulesOptions,
  type UpdateMatchingFunction,
  type WorkspacePackages,
} from '@pnpm/installing.deps-installer'
import { writeWantedLockfile } from '@pnpm/lockfile.fs'
import type { LockfileObject } from '@pnpm/lockfile.types'
import { globalInfo, logger } from '@pnpm/logger'
import { applyRuntimeOnFailOverride, filterDependenciesByType } from '@pnpm/pkg-manifest.utils'
import type { PreferredVersions, VersionSelectors } from '@pnpm/resolving.resolver-base'
import { createStoreController, type CreateStoreControllerOptions } from '@pnpm/store.connection-manager'
import type {
  IncludedDependencies,
  PackageVulnerabilityAudit,
  Project,
  ProjectRootDir,
  ProjectsGraph,
  VulnerabilitySeverity,
} from '@pnpm/types'
import { filterProjectsBySelectorObjects } from '@pnpm/workspace.projects-filter'
import { createProjectsGraph } from '@pnpm/workspace.projects-graph'
import { findWorkspaceProjects } from '@pnpm/workspace.projects-reader'
import { sequenceGraph } from '@pnpm/workspace.projects-sorter'
import { updateWorkspaceState, type WorkspaceStateSettings } from '@pnpm/workspace.state'
import { updateWorkspaceManifest } from '@pnpm/workspace.workspace-manifest-writer'

import { getPinnedVersion } from './getPinnedVersion.js'
import { getSaveType } from './getSaveType.js'
import { handleIgnoredBuilds } from './handleIgnoredBuilds.js'
import { setupPolicyHandlers } from './policyHandlers.js'
import {
  type CommandFullName,
  createMatcher,
  makeIgnorePatterns,
  matchDependencies,
  recursive,
  type RecursiveOptions,
  type UpdateDepsMatcher,
} from './recursive.js'
import { makeRunPacquet } from './runPacquet.js'
import { createWorkspaceSpecs, updateToWorkspacePackagesFromManifest } from './updateWorkspaceDependencies.js'
import { verifyPacquetIdentity } from './verifyPacquetIdentity.js'

const OVERWRITE_UPDATE_OPTIONS = {
  allowNew: true,
  update: false,
}

export type InstallDepsOptions = Pick<Config,
| 'autoInstallPeers'
| 'bail'
| 'bin'
| 'catalogs'
| 'catalogMode'
| 'cleanupUnusedCatalogs'
| 'dedupePeerDependents'
| 'dedupePeers'
| 'depth'
| 'dev'
| 'enableGlobalVirtualStore'
| 'virtualStoreOnly'
| 'engineStrict'
| 'excludeLinksFromLockfile'
| 'global'
| 'globalPnpmfile'
| 'ignoreCurrentSpecifiers'
| 'ignorePnpmfile'
| 'ignoreScripts'
| 'optimisticRepeatInstall'
| 'linkWorkspacePackages'
| 'lockfileDir'
| 'lockfileOnly'
| 'pnprServer'
| 'production'
| 'preferWorkspacePackages'
| 'registries'
| 'runtime'
| 'runtimeOnFail'
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
| 'sideEffectsCache'
| 'sideEffectsCacheReadonly'
| 'sort'
| 'sharedWorkspaceLockfile'
| 'shellEmulator'
| 'tag'
| 'trustLockfile'
| 'allowBuilds'
| 'optional'
| 'workspaceConcurrency'
| 'workspaceDir'
| 'workspacePackagePatterns'
| 'extraEnv'
| 'ignoreWorkspace'
| 'ignoreWorkspaceCycles'
| 'disallowWorkspaceCycles'
| 'configDependencies'
| 'packageExtensions'
| 'updateConfig'
| 'virtualStoreDirMaxLength'
> & Pick<ConfigContext,
| 'allProjects'
| 'allProjectsGraph'
| 'cliOptions'
| 'hooks'
| 'rootProjectManifestDir'
| 'rootProjectManifest'
| 'selectedProjectsGraph'
> & Partial<Pick<Config, 'ci'>>
& CreateStoreControllerOptions & {
  argv: {
    cooked?: string[]
    original: string[]
    remain?: string[]
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
  rebuildHandler?: CommandHandler
  pnpmfile: string[]
  packageVulnerabilityAudit?: PackageVulnerabilityAudit
  /**
   * `true` when this call originated from `pnpm install` (or `pnpm i`),
   * `false`/`undefined` for `add`, `update`, `dedupe`, etc. Used to gate
   * which pnpm CLI flags are safe to forward to pacquet's `install`
   * subcommand — see `runPacquet.ts`'s `noRuntime` opt.
   */
  isInstallCommand?: boolean
} & Partial<Pick<Config, 'dryRun' | 'pnpmHomeDir' | 'strictDepBuilds' | 'useLockfile' | 'useGitBranchLockfile' | 'mergeGitBranchLockfiles'>>

export async function installDeps (
  opts: InstallDepsOptions,
  params: string[]
): Promise<DryRunInstallResult | undefined> {
  if (!opts.update && !opts.dedupe && params.length === 0 && opts.optimisticRepeatInstall) {
    const { upToDate, wantedLockfileToRestore } = await checkDepsStatus({
      ...opts,
      ignoreFilteredInstallCache: true,
      treatLocalFileDepsAsOutdated: true,
    })
    if (upToDate && await restoreWantedLockfileIfMissing(wantedLockfileToRestore, opts)) {
      if (opts.hooks?.customResolvers?.some(r => r.shouldRefreshResolution)) {
        logger.warn({
          message: 'shouldRefreshResolution hooks were skipped because optimisticRepeatInstall is enabled.',
          prefix: opts.dir,
        })
      }
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
      opts.saveWorkspaceProtocol = true
    }
    // @ts-expect-error
    opts['preserveWorkspaceProtocol'] = !opts.linkWorkspacePackages
  }
  const store = await createStoreController(opts)
  // When `configDependencies` declares pacquet, build the alternative
  // install engine the deps-installer delegates to. The CLI layer owns
  // the construction so the installer doesn't need to know about
  // pacquet's binary path, CLI surface, or any settings that only
  // pacquet consumes. Threaded through both the workspace recursive
  // path and the single-project path below. Two declaration names are
  // accepted: the original unscoped `pacquet` and the official scoped
  // `@pnpm/pacquet` mirror. Both packages ship the same JS shim and
  // optional `@pacquet/<plat>-<arch>` binary sub-packages, so the
  // resolved \`node_modules/.pnpm-config/<name>\` layout pacquet's
  // wrapper expects is identical either way.
  //
  // `configDependencies` come from the repository's `pnpm-workspace.yaml`, so
  // the declaration cannot be trusted to authorize spawning a native binary on
  // its own. `verifyPacquetIdentity` confirms, against the canonical npm
  // registry, that the installed bytes carry a valid registry signature for
  // that `name@version` before we delegate; otherwise we fall back to pnpm's
  // own engine.
  const declaredPacquetConfigDepName = opts.configDependencies?.['@pnpm/pacquet'] != null
    ? '@pnpm/pacquet'
    : opts.configDependencies?.pacquet != null
      ? 'pacquet'
      : undefined
  const pacquetConfigDepName = declaredPacquetConfigDepName != null &&
    await verifyPacquetIdentity(declaredPacquetConfigDepName, {
      ...opts,
      lockfileDir: opts.lockfileDir ?? opts.dir,
      rootDir: opts.lockfileDir ?? opts.dir,
    })
    ? declaredPacquetConfigDepName
    : undefined
  const runPacquet = pacquetConfigDepName != null
    ? makeRunPacquet({
      lockfileDir: opts.lockfileDir ?? opts.dir,
      packageName: pacquetConfigDepName,
      argv: { original: opts.argv.original, remain: opts.argv.remain ?? [] },
      isInstallCommand: opts.isInstallCommand === true,
      virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
    })
    : undefined
  const includeDirect = opts.includeDirect ?? {
    dependencies: true,
    devDependencies: true,
    optionalDependencies: true,
  }
  const allProjects = opts.allProjects ?? (
    opts.workspaceDir
      ? await findWorkspaceProjects(opts.workspaceDir, { ...opts, patterns: opts.workspacePackagePatterns })
      : []
  )
  if (opts.runtimeOnFail) {
    for (const project of allProjects) {
      applyRuntimeOnFailOverride(project.manifest, opts.runtimeOnFail)
    }
  }
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

      const allProjectsGraph: ProjectsGraph = opts.allProjectsGraph ?? createProjectsGraph(allProjects, {
        linkWorkspacePackages: Boolean(opts.linkWorkspacePackages),
      }).graph

      return recursiveInstallThenUpdateWorkspaceState(allProjects,
        params,
        {
          ...opts,
          preferredVersions: opts.packageVulnerabilityAudit ? preferNonvulnerablePackageVersions(opts.packageVulnerabilityAudit) : undefined,
          allProjectsGraph,
          selectedProjectsGraph,
          storeControllerAndDir: store,
          workspaceDir: opts.workspaceDir,
          runPacquet,
        },
        opts.update ? 'update' : (params.length === 0 ? 'install' : 'add')
      )
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
  } else if (opts.runtimeOnFail) {
    applyRuntimeOnFailOverride(manifest, opts.runtimeOnFail)
  }

  // `setupPolicyHandlers` composes the per-policy handlers the install
  // needs for the current opts (today: minimumReleaseAge; future:
  // trustPolicy UX, license policy, etc.). Returns `undefined` when no
  // handler is active so the install skips the empty no-op call at
  // every checkpoint when no policies are configured.
  const policyHandlers = setupPolicyHandlers(opts)

  const installOpts: Omit<MutateModulesOptions, 'allProjects'> = {
    ...opts,
    // In case installation is done in a multi-package repository
    // The dependencies should be built first,
    // so ignoring scripts for now
    ignoreScripts: !!workspacePackages || opts.ignoreScripts,
    linkWorkspacePackagesDepth: opts.linkWorkspacePackages === 'deep' ? Infinity : opts.linkWorkspacePackages ? 0 : -1,
    sideEffectsCacheRead: opts.sideEffectsCache ?? opts.sideEffectsCacheReadonly,
    sideEffectsCacheWrite: opts.sideEffectsCache,
    skipRuntimes: opts.runtime === false,
    storeController: store.ctrl,
    storeDir: store.dir,
    resolutionVerifiers: store.resolutionVerifiers,
    workspacePackages,
    preferredVersions: opts.packageVulnerabilityAudit ? preferNonvulnerablePackageVersions(opts.packageVulnerabilityAudit) : undefined,
    handleResolutionPolicyViolations: policyHandlers?.handleResolutionPolicyViolations,
    runPacquet,
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
    const { updatedCatalogs, updatedProject, ignoredBuilds, resolutionPolicyViolations, dryRunResult } = await mutateModulesInSingleProject(mutatedProject, installOpts)
    if (opts.save !== false && !opts.dryRun) {
      // Only pick entries when we'll actually persist. Otherwise the
      // info log would claim we added entries the workspace manifest
      // never saw, and the next install would re-prompt or fail
      // verification.
      const policyUpdates = policyHandlers?.pickManifestUpdates(resolutionPolicyViolations)
      await Promise.all([
        writeProjectManifest(updatedProject.manifest),
        updateWorkspaceManifest(opts.workspaceDir ?? opts.dir, {
          updatedCatalogs,
          cleanupUnusedCatalogs: opts.cleanupUnusedCatalogs,
          allProjects: opts.allProjects,
          ...policyUpdates,
        }),
      ])
    }
    if (!opts.lockfileOnly) {
      await updateWorkspaceState({
        allProjects,
        settings: withUpdatedCatalogs(opts, updatedCatalogs),
        workspaceDir: opts.workspaceDir ?? opts.lockfileDir ?? opts.dir,
        pnpmfiles: opts.pnpmfile,
        filteredInstall: allProjects.length !== Object.keys(opts.selectedProjectsGraph ?? {}).length,
        configDependencies: opts.configDependencies,
      })
    }
    await handleIgnoredBuilds(opts, ignoredBuilds)
    return dryRunResult
  }

  const { updatedCatalogs, updatedManifest, ignoredBuilds, resolutionPolicyViolations, dryRunResult } = await install(manifest, {
    ...installOpts,
    updatePackageManifest,
    updateMatching,
  })
  // `opts.save === false` (e.g. `--no-save`) means "don't persist anything
  // from this install" — both package.json and the workspace manifest.
  // Skip the pick so the info log doesn't claim entries were added that
  // were never written; the next install will resurface them.
  if (opts.save !== false && !opts.dryRun) {
    const policyUpdates = policyHandlers?.pickManifestUpdates(resolutionPolicyViolations)
    if (opts.update === true) {
      await Promise.all([
        writeProjectManifest(updatedManifest),
        updateWorkspaceManifest(opts.workspaceDir ?? opts.dir, {
          updatedCatalogs,
          cleanupUnusedCatalogs: opts.cleanupUnusedCatalogs,
          allProjects,
          ...policyUpdates,
        }),
      ])
    } else if (policyUpdates != null) {
      // Plain `pnpm install` (no --update, no params) wouldn't otherwise touch
      // the workspace manifest. Persist the auto-policy patches anyway so any
      // loose bypass (today: minimumReleaseAgeExclude) remains explicit on
      // subsequent installs.
      await updateWorkspaceManifest(opts.workspaceDir ?? opts.dir, policyUpdates)
    }
  }
  await handleIgnoredBuilds(opts, ignoredBuilds)

  if (opts.linkWorkspacePackages && opts.workspaceDir) {
    const { selectedProjectsGraph } = await filterProjectsBySelectorObjects(allProjects, [
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
      runPacquet,
    }, 'install', updatedCatalogs)

    if (opts.ignoreScripts) return

    await buildProjects(
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
        settings: withUpdatedCatalogs(opts, updatedCatalogs),
        workspaceDir: opts.workspaceDir ?? opts.lockfileDir ?? opts.dir,
        pnpmfiles: opts.pnpmfile,
        filteredInstall: allProjects.length !== Object.keys(opts.selectedProjectsGraph ?? {}).length,
        configDependencies: opts.configDependencies,
      })
    }
  }
  return dryRunResult
}

function selectProjectByDir (projects: Project[], searchedDir: string): ProjectsGraph | undefined {
  const project = projects.find(({ rootDir }) => path.relative(rootDir, searchedDir) === '')
  if (project == null) return undefined
  return { [project.rootDir]: { dependencies: [], package: project } }
}

async function recursiveInstallThenUpdateWorkspaceState (
  allProjects: Project[],
  params: string[],
  opts: RecursiveOptions & WorkspaceStateSettings,
  cmdFullName: CommandFullName,
  updatedCatalogs?: Catalogs
): Promise<DryRunInstallResult | undefined> {
  const recursiveResult = await recursive(allProjects, params, opts, cmdFullName)
  if (!opts.lockfileOnly) {
    await updateWorkspaceState({
      allProjects,
      settings: withUpdatedCatalogs(opts, updatedCatalogs, recursiveResult.updatedCatalogs),
      workspaceDir: opts.workspaceDir,
      pnpmfiles: opts.pnpmfile,
      filteredInstall: allProjects.length !== Object.keys(opts.selectedProjectsGraph ?? {}).length,
      configDependencies: opts.configDependencies,
    })
  }
  return recursiveResult.dryRunResult
}

/**
 * Folds the catalog entries written to `pnpm-workspace.yaml` during this
 * install into the catalogs read at startup. The workspace state cache records
 * these so a later install detects when a catalog entry was reverted; without
 * this, the cache would keep the stale pre-install catalogs and report
 * "Already up to date" even though the manifest changed.
 */
function withUpdatedCatalogs<T extends { catalogs?: Catalogs }> (
  settings: T,
  ...updatedCatalogs: Array<Catalogs | undefined>
): T {
  if (updatedCatalogs.every((catalogs) => catalogs == null)) return settings
  return { ...settings, catalogs: mergeCatalogs(settings.catalogs, ...updatedCatalogs) }
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
    case 'low': return -1100 // 100 more than DIRECT_DEP_SELECTOR_WEIGHT from @pnpm/resolving.resolver-base
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

/**
 * Restore a missing `pnpm-lock.yaml` from the current lockfile before the
 * optimistic repeat-install short-circuit reports "Already up to date", so
 * the fast path leaves the same on-disk contract a full install would.
 * Returns `true` when the short-circuit may proceed: nothing to restore,
 * lockfile writing is disabled (`useLockfile: false`), or the restore
 * succeeded. A failed write returns `false` so the caller falls through to
 * the full install instead of reporting up to date while `pnpm-lock.yaml`
 * stays missing.
 */
async function restoreWantedLockfileIfMissing (
  wantedLockfileToRestore: { lockfile: LockfileObject, lockfileDir: string } | undefined,
  opts: Pick<InstallDepsOptions, 'useLockfile'>
): Promise<boolean> {
  if (wantedLockfileToRestore == null || opts.useLockfile === false) return true
  try {
    await writeWantedLockfile(wantedLockfileToRestore.lockfileDir, wantedLockfileToRestore.lockfile)
    return true
  } catch (error) {
    logger.debug({ msg: 'Failed to restore pnpm-lock.yaml from the current lockfile', error })
    return false
  }
}
