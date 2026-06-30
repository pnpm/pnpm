import fs from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import { resolveFromCatalog } from '@pnpm/catalogs.resolver'
import type { Catalogs } from '@pnpm/catalogs.types'
import { parseOverrides } from '@pnpm/config.parse-overrides'
import type { Config, ConfigContext } from '@pnpm/config.reader'
import { MANIFEST_BASE_NAMES } from '@pnpm/constants'
import { hashObjectNullableWithPrefix } from '@pnpm/crypto.object-hasher'
import { PnpmError } from '@pnpm/error'
import { arrayOfWorkspacePackagesToMap } from '@pnpm/installing.context'
import {
  getGitBranchLockfileNamesSync,
  getLockfileImporterId,
  getWantedLockfileName,
  type LockfileObject,
  readCurrentLockfile,
  readWantedLockfile,
  wantedLockfileHasMergeConflictsSync,
} from '@pnpm/lockfile.fs'
import {
  calcPatchHashes,
  createOverridesMapFromParsed,
  getOutdatedLockfileSetting,
} from '@pnpm/lockfile.settings-checker'
import {
  getWorkspacePackagesByDirectory,
  linkedPackagesAreUpToDate,
  satisfiesPackageManifest,
} from '@pnpm/lockfile.verification'
import { globalWarn, logger } from '@pnpm/logger'
import type { WorkspacePackages } from '@pnpm/resolving.resolver-base'
import {
  DEPENDENCIES_FIELDS,
  type DependencyManifest,
  type IncludedDependencies,
  type Project,
  type ProjectId,
  type ProjectManifest,
} from '@pnpm/types'
import { findWorkspaceProjectsNoCheck } from '@pnpm/workspace.projects-reader'
import { loadWorkspaceState, updateWorkspaceState, WORKSPACE_STATE_SETTING_KEYS, type WorkspaceState, type WorkspaceStateSettings } from '@pnpm/workspace.state'
import { readWorkspaceManifest } from '@pnpm/workspace.workspace-manifest-reader'
import { equals, filter, isEmpty, once } from 'ramda'

import { assertLockfilesEqual } from './assertLockfilesEqual.js'
import { safeStat, safeStatSync } from './safeStat.js'
import { statManifestFile } from './statManifestFile.js'

export type CheckDepsStatusOptions = Pick<Config,
| 'autoInstallPeers'
| 'catalogs'
| 'excludeLinksFromLockfile'
| 'injectWorkspacePackages'
| 'linkWorkspacePackages'
| 'lockfileDir'
| 'mergeGitBranchLockfiles'
| 'nodeLinker'
| 'patchedDependencies'
| 'peersSuffixMaxLength'
| 'sharedWorkspaceLockfile'
| 'workspaceDir'
| 'patchesDir'
| 'configDependencies'
| 'overrides'
| 'packageExtensions'
| 'ignoredOptionalDependencies'
> & Pick<ConfigContext,
| 'allProjects'
| 'hooks'
| 'rootProjectManifest'
| 'rootProjectManifestDir'
> & {
  ignoreFilteredInstallCache?: boolean
  ignoredWorkspaceStateSettings?: Array<keyof WorkspaceStateSettings>
  pnpmfile: string[]
  /**
   * The checks below only track manifest and lockfile mtimes, so edits inside
   * a local file dependency's directory (or a repacked local tarball) go
   * unnoticed. Callers that skip the install entirely when this check reports
   * up-to-date must set this so that projects with local file dependencies
   * (`file:` and bare local path/tarball specifiers) always run a real
   * install, which refetches those dependencies
   * (https://github.com/pnpm/pnpm/issues/11795).
   */
  treatLocalFileDepsAsOutdated?: boolean
  /**
   * Which dependency groups the current install materializes. Local file
   * dependencies in an excluded group (for example `devDependencies` under
   * `--prod`) are not installed, so they don't force the
   * `treatLocalFileDepsAsOutdated` bail-out. A change to these flags between
   * installs is caught separately by the workspace state settings comparison
   * (`dev`/`optional`/`production` are part of
   * `WORKSPACE_STATE_SETTING_KEYS`).
   */
  include?: IncludedDependencies
  /**
   * When git-branch lockfiles are enabled, the wanted lockfile lives at
   * `pnpm-lock.<branch>.yaml`, so a missing `pnpm-lock.yaml` is the steady
   * state — the current-lockfile stand-in must not kick in.
   */
  useGitBranchLockfile?: boolean
} & WorkspaceStateSettings

export interface CheckDepsStatusResult {
  upToDate: boolean | undefined
  issue?: string
  workspaceState: WorkspaceState | undefined
  /**
   * Set when `pnpm-lock.yaml` was missing and the current lockfile
   * (`<lockfileDir>/node_modules/.pnpm/lock.yaml`) stood in as the wanted
   * lockfile for the up-to-date checks. The current lockfile records
   * exactly what the previous install materialized, so the caller can
   * restore `pnpm-lock.yaml` from it without resolving — `installDeps`
   * does that before reporting "Already up to date".
   */
  wantedLockfileToRestore?: {
    lockfile: LockfileObject
    lockfileDir: string
  }
}

export async function checkDepsStatus (opts: CheckDepsStatusOptions): Promise<CheckDepsStatusResult> {
  const workspaceState = loadWorkspaceState(opts.workspaceDir ?? opts.rootProjectManifestDir)
  if (!workspaceState) {
    return {
      upToDate: false,
      issue: 'Cannot check whether dependencies are outdated',
      workspaceState,
    }
  }
  try {
    return await _checkDepsStatus(opts, workspaceState)
  } catch (error) {
    if (util.types.isNativeError(error) && 'code' in error && String(error.code).startsWith('ERR_PNPM_RUN_CHECK_DEPS_')) {
      return {
        upToDate: false,
        issue: error.message,
        workspaceState,
      }
    }
    // This function never throws an error.
    // We want to ensure that pnpm CLI never crashes when checking the status of dependencies.
    // In the worst-case scenario, the install will run redundantly.
    return {
      upToDate: undefined,
      issue: util.types.isNativeError(error) ? error.message : undefined,
      workspaceState,
    }
  }
}

async function _checkDepsStatus (opts: CheckDepsStatusOptions, workspaceState: WorkspaceState): Promise<CheckDepsStatusResult> {
  const {
    allProjects,
    autoInstallPeers,
    injectWorkspacePackages,
    catalogs,
    excludeLinksFromLockfile,
    linkWorkspacePackages,
    lockfileDir,
    nodeLinker,
    patchedDependencies,
    rootProjectManifest,
    rootProjectManifestDir,
    sharedWorkspaceLockfile,
    workspaceDir,
  } = opts

  // This check must run before the node-linker=pnp early return below:
  // that return reports up-to-date because verify-deps-before-run cannot
  // inspect a PnP install, but for the optimistic repeat-install caller
  // (the only one setting this flag) "up-to-date" would skip the install
  // and break the local-file-deps guarantee.
  if (opts.treatLocalFileDepsAsOutdated) {
    const manifests = allProjects?.map(({ manifest }) => manifest) ?? []
    // `rootProjectManifest` is tracked separately from `allProjects` and the
    // recursive project list can omit the workspace root (for example when
    // `includeWorkspaceRoot` is false), so scan it too unless `allProjects`
    // already covers it.
    if (rootProjectManifest != null && !allProjects?.some(({ rootDir }) => rootDir === rootProjectManifestDir)) {
      manifests.push(rootProjectManifest)
    }
    const localFileDep = findLocalFileDep(manifests, opts.include, catalogs)
    if (localFileDep != null) {
      return {
        upToDate: false,
        issue: `The dependency "${localFileDep}" is a local file dependency and its contents may have changed`,
        workspaceState,
      }
    }
    const localFileOverride = findLocalFileOverride(opts.overrides, catalogs)
    if (localFileOverride != null) {
      return {
        upToDate: false,
        issue: `The override "${localFileOverride}" maps to a local file dependency and its contents may have changed`,
        workspaceState,
      }
    }
    const localFileExtension = findLocalFilePackageExtension(opts.packageExtensions, opts.include, catalogs)
    if (localFileExtension != null) {
      return {
        upToDate: false,
        issue: `The package extension "${localFileExtension}" injects a local file dependency and its contents may have changed`,
        workspaceState,
      }
    }
  }

  if (nodeLinker === 'pnp') {
    globalWarn('verify-deps-before-run does not work with node-linker=pnp')
    return { upToDate: true, workspaceState: undefined }
  }

  if (opts.ignoreFilteredInstallCache && workspaceState.filteredInstall) {
    return { upToDate: undefined, workspaceState }
  }

  if (workspaceState.settings) {
    const ignoredSettings = new Set<keyof WorkspaceStateSettings>(opts.ignoredWorkspaceStateSettings)
    ignoredSettings.add('catalogs')
    for (const settingName of WORKSPACE_STATE_SETTING_KEYS) {
      if (ignoredSettings.has(settingName as keyof WorkspaceStateSettings)) continue
      const storedValue = settingName === 'allowBuilds'
        ? workspaceState.settings[settingName] ?? {}
        : workspaceState.settings[settingName as keyof WorkspaceStateSettings]
      const currentValue = settingName === 'allowBuilds'
        ? opts.allowBuilds ?? {}
        : opts[settingName as keyof WorkspaceStateSettings]
      if (!equals(storedValue, currentValue)) {
        return {
          upToDate: false,
          issue: `The value of the ${settingName} setting has changed`,
          workspaceState,
        }
      }
    }
  }
  if ((opts.configDependencies != null || workspaceState.configDependencies != null) && !equals(opts.configDependencies ?? {}, workspaceState.configDependencies ?? {})) {
    return {
      upToDate: false,
      issue: 'Configuration dependencies are not up to date',
      workspaceState,
    }
  }

  const lockfileDirs = getWantedLockfileDirs({
    allProjects,
    lockfileDir,
    rootProjectManifestDir,
    sharedWorkspaceLockfile,
    workspaceDir,
  })
  const wantedLockfileName = await getWantedLockfileName({
    useGitBranchLockfile: opts.useGitBranchLockfile,
    mergeGitBranchLockfiles: opts.mergeGitBranchLockfiles,
    cwd: workspaceDir ?? lockfileDir ?? rootProjectManifestDir,
  })
  const { conflictedDir: conflictedLockfileDir, anyModified: lockfilesModified, anyMissing: lockfilesMissing } = scanWantedLockfiles(lockfileDirs, workspaceState.lastValidatedTimestamp, {
    wantedLockfileName,
    mergeGitBranchLockfiles: opts.mergeGitBranchLockfiles,
  })
  if (conflictedLockfileDir != null) {
    return {
      upToDate: false,
      issue: `The lockfile in ${conflictedLockfileDir} has merge conflicts`,
      workspaceState,
    }
  }

  if (allProjects && workspaceDir) {
    if (!equals(
      filter(value => value != null, workspaceState.settings.catalogs ?? {}),
      filter(value => value != null, catalogs ?? {})
    )) {
      return {
        upToDate: false,
        issue: 'Catalogs cache outdated',
        workspaceState,
      }
    }

    if (allProjects.length !== Object.keys(workspaceState.projects).length ||
      !allProjects.every((currentProject) => {
        const prevProject = workspaceState.projects[currentProject.rootDir]
        if (!prevProject) return false
        return prevProject.name === currentProject.manifest.name && (prevProject.version ?? '0.0.0') === (currentProject.manifest.version ?? '0.0.0')
      })
    ) {
      return {
        upToDate: false,
        issue: 'The workspace structure has changed since last install',
        workspaceState,
      }
    }

    let statModulesDir: (project: Project) => Promise<fs.Stats | undefined>
    if (nodeLinker === 'hoisted') {
      const statsPromise = safeStat(path.join(rootProjectManifestDir, 'node_modules'))
      statModulesDir = () => statsPromise
    } else {
      const _nodeLinkerTypeGuard: 'isolated' | undefined = nodeLinker // static type assertion
      statModulesDir = project => safeStat(path.join(project.rootDir, 'node_modules'))
    }

    const allManifestStats = await Promise.all(allProjects.map(async project => {
      const modulesDirStatsPromise = statModulesDir(project)
      const manifestStats = await statManifestFile(project.rootDir)
      if (!manifestStats) {
        // this error should not happen
        throw new Error(`Cannot find one of ${MANIFEST_BASE_NAMES.join(', ')} in ${project.rootDir}`)
      }
      return {
        project,
        manifestStats,
        modulesDirStats: await modulesDirStatsPromise,
      }
    }))

    if (!workspaceState.filteredInstall) {
      for (const { modulesDirStats, project } of allManifestStats) {
        if (modulesDirStats) continue
        if (isEmpty({
          ...project.manifest.dependencies,
          ...project.manifest.devDependencies,
        })) continue
        const id = project.manifest.name ?? project.rootDir
        return {
          upToDate: false,
          issue: `Workspace package ${id} has dependencies but does not have a modules directory`,
          workspaceState,
        }
      }
    }

    const issue = await patchesOrHooksAreModified({
      patchedDependencies,
      rootDir: rootProjectManifestDir,
      lastValidatedTimestamp: workspaceState.lastValidatedTimestamp,
      currentPnpmfiles: opts.pnpmfile,
      previousPnpmfiles: workspaceState.pnpmfiles,
    })
    if (issue) {
      return { upToDate: false, issue, workspaceState }
    }

    const modifiedProjects = allManifestStats.filter(
      ({ manifestStats }) =>
        manifestStats.mtime.valueOf() > workspaceState.lastValidatedTimestamp
    )

    if ((modifiedProjects.length === 0) && !lockfilesModified) {
      const wantedLockfileToRestore = lockfilesMissing && sharedWorkspaceLockfile && !opts.useGitBranchLockfile
        ? await missingWantedLockfileStandIn(workspaceDir, wantedLockfileName)
        : undefined
      // A missing wanted lockfile only skips the full check when the current
      // lockfile can stand in for it. Otherwise fall through so the checks
      // below throw RUN_CHECK_DEPS_LOCKFILE_NOT_FOUND instead of silently
      // reporting "up to date".
      if (!lockfilesMissing || wantedLockfileToRestore != null) {
        logger.debug({ msg: 'No manifest files or lockfiles were modified since the last validation. Exiting check.' })
        return { upToDate: true, workspaceState, wantedLockfileToRestore }
      }
    }

    logger.debug({ msg: 'Some manifest files or lockfiles were modified since the last validation. Continuing check.' })

    let wantedLockfileToRestore: CheckDepsStatusResult['wantedLockfileToRestore']
    let readWantedLockfileAndDir: (projectDir: string) => Promise<{
      wantedLockfile: LockfileObject
      wantedLockfileDir: string
    }>
    if (sharedWorkspaceLockfile) {
      let wantedLockfileStats: fs.Stats | undefined
      try {
        wantedLockfileStats = fs.statSync(path.join(workspaceDir, wantedLockfileName))
      } catch (error) {
        if (util.types.isNativeError(error) && 'code' in error && error.code === 'ENOENT') {
          wantedLockfileStats = undefined
        } else {
          throw error
        }
      }

      if (wantedLockfileStats == null) {
        // `pnpm-lock.yaml` is gone, but the current lockfile records
        // exactly what the previous install materialized — let it stand
        // in as the wanted lockfile for the checks below, and report it
        // back so `installDeps` can restore `pnpm-lock.yaml` from it
        // without resolving. There is no second lockfile to compare
        // against, so the wanted-vs-current equality assertion doesn't
        // apply on this path.
        if (opts.useGitBranchLockfile) return throwLockfileNotFound(workspaceDir)
        const currentLockfile = await readCurrentLockfile(path.join(workspaceDir, 'node_modules/.pnpm'), { ignoreIncompatible: false })
        if (currentLockfile == null) return throwLockfileNotFound(workspaceDir)
        wantedLockfileToRestore = { lockfile: currentLockfile, lockfileDir: workspaceDir }
        readWantedLockfileAndDir = async () => ({
          wantedLockfile: currentLockfile,
          wantedLockfileDir: workspaceDir,
        })
      } else {
        const wantedLockfilePromise = readWantedLockfile(workspaceDir, {
          ignoreIncompatible: false,
          useGitBranchLockfile: opts.useGitBranchLockfile,
          mergeGitBranchLockfiles: opts.mergeGitBranchLockfiles,
        })
        if (wantedLockfileStats.mtime.valueOf() > workspaceState.lastValidatedTimestamp) {
          const currentLockfile = await readCurrentLockfile(path.join(workspaceDir, 'node_modules/.pnpm'), { ignoreIncompatible: false })
          const wantedLockfile = (await wantedLockfilePromise) ?? throwLockfileNotFound(workspaceDir)
          assertLockfilesEqual(currentLockfile, wantedLockfile, workspaceDir)
        }
        readWantedLockfileAndDir = async () => ({
          wantedLockfile: (await wantedLockfilePromise) ?? throwLockfileNotFound(workspaceDir),
          wantedLockfileDir: workspaceDir,
        })
      }
    } else {
      readWantedLockfileAndDir = async wantedLockfileDir => {
        const wantedLockfilePromise = readWantedLockfile(wantedLockfileDir, {
          ignoreIncompatible: false,
          useGitBranchLockfile: opts.useGitBranchLockfile,
          mergeGitBranchLockfiles: opts.mergeGitBranchLockfiles,
        })
        const wantedLockfileStats = await safeStat(path.join(wantedLockfileDir, wantedLockfileName))

        if (!wantedLockfileStats) return throwLockfileNotFound(wantedLockfileDir)
        if (wantedLockfileStats.mtime.valueOf() > workspaceState.lastValidatedTimestamp) {
          const currentLockfile = await readCurrentLockfile(path.join(wantedLockfileDir, 'node_modules/.pnpm'), { ignoreIncompatible: false })
          const wantedLockfile = (await wantedLockfilePromise) ?? throwLockfileNotFound(wantedLockfileDir)
          assertLockfilesEqual(currentLockfile, wantedLockfile, wantedLockfileDir)
        }

        return {
          wantedLockfile: (await wantedLockfilePromise) ?? throwLockfileNotFound(wantedLockfileDir),
          wantedLockfileDir,
        }
      }
    }

    type GetProjectId = (project: Pick<Project, 'rootDir'>) => ProjectId
    const getProjectId: GetProjectId = sharedWorkspaceLockfile
      ? project => getLockfileImporterId(workspaceDir, project.rootDir)
      : () => '.' as ProjectId

    const getWorkspacePackages = once(arrayOfWorkspacePackagesToMap.bind(null, allProjects))
    const getManifestsByDir = once(() => getWorkspacePackagesByDirectory(getWorkspacePackages()))

    const assertCtx: AssertWantedLockfileUpToDateContext = {
      autoInstallPeers,
      injectWorkspacePackages,
      config: opts,
      excludeLinksFromLockfile,
      linkWorkspacePackages,
      getManifestsByDir,
      getWorkspacePackages,
      rootDir: workspaceDir,
    }

    try {
      const projectsToCheck = lockfilesModified ? allManifestStats : modifiedProjects
      await Promise.all(projectsToCheck.map(async ({ project }) => {
        const { wantedLockfile, wantedLockfileDir } = await readWantedLockfileAndDir(project.rootDir)
        await assertWantedLockfileUpToDate(assertCtx, {
          projectDir: project.rootDir,
          projectId: getProjectId(project),
          projectManifest: project.manifest,
          wantedLockfile,
          wantedLockfileDir,
        })
      }))
    } catch (err) {
      return {
        upToDate: false,
        issue: (util.types.isNativeError(err) && 'message' in err) ? err.message : undefined,
        workspaceState,
      }
    }

    // update lastValidatedTimestamp to prevent pointless repeat
    await updateWorkspaceState({
      allProjects,
      workspaceDir,
      pnpmfiles: workspaceState.pnpmfiles,
      settings: opts,
      filteredInstall: workspaceState.filteredInstall,
    })

    return { upToDate: true, workspaceState, wantedLockfileToRestore }
  }

  if (!allProjects) {
    const workspaceRoot = workspaceDir ?? rootProjectManifestDir
    const workspaceManifest = await readWorkspaceManifest(workspaceRoot)
    if (workspaceManifest ?? workspaceDir) {
      const allProjects = await findWorkspaceProjectsNoCheck(rootProjectManifestDir, {
        patterns: workspaceManifest?.packages,
      })
      return checkDepsStatus({
        ...opts,
        allProjects,
      })
    }
  } else {
    // this error shouldn't happen
    throw new Error('Impossible variant: allProjects is defined but workspaceDir is undefined')
  }

  if (rootProjectManifest && rootProjectManifestDir) {
    const internalPnpmDir = path.join(rootProjectManifestDir, 'node_modules', '.pnpm')
    const currentLockfilePromise = readCurrentLockfile(internalPnpmDir, { ignoreIncompatible: false })
    const wantedLockfilePromise = readWantedLockfile(rootProjectManifestDir, {
      ignoreIncompatible: false,
      useGitBranchLockfile: opts.useGitBranchLockfile,
      mergeGitBranchLockfiles: opts.mergeGitBranchLockfiles,
    })
    const [
      currentLockfileStats,
      wantedLockfileStats,
      manifestStats,
    ] = await Promise.all([
      safeStat(path.join(internalPnpmDir, 'lock.yaml')),
      safeStat(path.join(rootProjectManifestDir, wantedLockfileName)),
      statManifestFile(rootProjectManifestDir),
    ])

    if (!wantedLockfileStats && (!currentLockfileStats || opts.useGitBranchLockfile)) return throwLockfileNotFound(rootProjectManifestDir)

    // When `pnpm-lock.yaml` is gone but the current lockfile
    // (`node_modules/.pnpm/lock.yaml`) survives, the current one stands
    // in as the wanted lockfile: it records exactly what the previous
    // install materialized, so the checks below run against it and the
    // caller can restore `pnpm-lock.yaml` from it without resolving.
    // The wanted-vs-current equality assertion doesn't apply on this
    // path — the two are the same object.
    const wantedLockfileIsMissing = !wantedLockfileStats
    const effectiveWantedLockfileStats = (wantedLockfileStats ?? currentLockfileStats)!
    const readEffectiveWantedLockfile = async (): Promise<LockfileObject> => {
      const lockfile = wantedLockfileIsMissing ? await currentLockfilePromise : await wantedLockfilePromise
      return lockfile ?? throwLockfileNotFound(rootProjectManifestDir)
    }

    const issue = await patchesOrHooksAreModified({
      patchedDependencies,
      rootDir: rootProjectManifestDir,
      lastValidatedTimestamp: effectiveWantedLockfileStats.mtime.valueOf(),
      currentPnpmfiles: opts.pnpmfile,
      previousPnpmfiles: workspaceState.pnpmfiles,
    })
    if (issue) {
      return { upToDate: false, issue, workspaceState }
    }

    if (!wantedLockfileIsMissing && currentLockfileStats && wantedLockfileStats.mtime.valueOf() > currentLockfileStats.mtime.valueOf()) {
      const currentLockfile = await currentLockfilePromise
      const wantedLockfile = (await wantedLockfilePromise) ?? throwLockfileNotFound(rootProjectManifestDir)
      assertLockfilesEqual(currentLockfile, wantedLockfile, rootProjectManifestDir)
    }

    if (!manifestStats) {
      // this error should not happen
      throw new Error(`Cannot find one of ${MANIFEST_BASE_NAMES.join(', ')} in ${rootProjectManifestDir}`)
    }

    if (manifestStats.mtime.valueOf() > effectiveWantedLockfileStats.mtime.valueOf()) {
      logger.debug({ msg: 'The manifest is newer than the lockfile. Continuing check.' })
      try {
        await assertWantedLockfileUpToDate({
          autoInstallPeers,
          injectWorkspacePackages,
          config: opts,
          excludeLinksFromLockfile,
          linkWorkspacePackages,
          getManifestsByDir: () => ({}),
          getWorkspacePackages: () => undefined,
          rootDir: rootProjectManifestDir,
        }, {
          projectDir: rootProjectManifestDir,
          projectId: '.' as ProjectId,
          projectManifest: rootProjectManifest,
          wantedLockfile: await readEffectiveWantedLockfile(),
          wantedLockfileDir: rootProjectManifestDir,
        })
      } catch (err) {
        return {
          upToDate: false,
          issue: (util.types.isNativeError(err) && 'message' in err) ? err.message : undefined,
          workspaceState,
        }
      }
    } else if (currentLockfileStats) {
      logger.debug({ msg: 'The manifest file is not newer than the lockfile. Exiting check.' })
    } else {
      const wantedLockfile = (await wantedLockfilePromise) ?? throwLockfileNotFound(rootProjectManifestDir)
      if (!isEmpty(wantedLockfile.packages ?? {})) {
        throw new PnpmError('RUN_CHECK_DEPS_NO_DEPS', 'The lockfile requires dependencies but none were installed', {
          hint: 'Run `pnpm install` to install dependencies',
        })
      }
    }

    if (wantedLockfileIsMissing) {
      const currentLockfile = await currentLockfilePromise
      if (currentLockfile != null) {
        return {
          upToDate: true,
          workspaceState,
          wantedLockfileToRestore: { lockfile: currentLockfile, lockfileDir: rootProjectManifestDir },
        }
      }
    }
    return { upToDate: true, workspaceState }
  }

  // `opts.allProject` being `undefined` means that the run command was not run with `--recursive`.
  // `rootProjectManifest` being `undefined` means that there's no root manifest.
  // Both means that `pnpm run` would fail, so checking lockfiles here is pointless.
  globalWarn('Skipping check.')
  return { upToDate: undefined, workspaceState }
}

interface AssertWantedLockfileUpToDateContext {
  autoInstallPeers?: boolean
  config: CheckDepsStatusOptions
  excludeLinksFromLockfile?: boolean
  injectWorkspacePackages?: boolean
  linkWorkspacePackages: boolean | 'deep'
  getManifestsByDir: () => Record<string, DependencyManifest>
  getWorkspacePackages: () => WorkspacePackages | undefined
  rootDir: string
  patchedDependencies?: Record<string, string>
}

interface AssertWantedLockfileUpToDateOptions {
  projectDir: string
  projectId: ProjectId
  projectManifest: ProjectManifest
  wantedLockfile: LockfileObject
  wantedLockfileDir: string
}

async function assertWantedLockfileUpToDate (
  ctx: AssertWantedLockfileUpToDateContext,
  opts: AssertWantedLockfileUpToDateOptions
): Promise<void> {
  const {
    autoInstallPeers,
    config,
    excludeLinksFromLockfile,
    linkWorkspacePackages,
    getManifestsByDir,
    getWorkspacePackages,
  } = ctx

  const {
    projectDir,
    projectId,
    projectManifest,
    wantedLockfile,
    wantedLockfileDir,
  } = opts

  const [
    patchedDependencies,
    pnpmfileChecksum,
  ] = await Promise.all([
    calcPatchHashes(config.patchedDependencies ?? {}),
    config.hooks?.calculatePnpmfileChecksum?.(),
  ])

  const outdatedLockfileSettingName = getOutdatedLockfileSetting(wantedLockfile, {
    catalogs: config.catalogs,
    autoInstallPeers: config.autoInstallPeers,
    injectWorkspacePackages: config.injectWorkspacePackages,
    excludeLinksFromLockfile: config.excludeLinksFromLockfile,
    peersSuffixMaxLength: config.peersSuffixMaxLength,
    overrides: createOverridesMapFromParsed(parseOverrides(config.overrides ?? {}, config.catalogs)),
    ignoredOptionalDependencies: config.ignoredOptionalDependencies?.sort(),
    packageExtensionsChecksum: hashObjectNullableWithPrefix(config.packageExtensions),
    patchedDependencies,
    pnpmfileChecksum,
  })

  if (outdatedLockfileSettingName) {
    throw new PnpmError('RUN_CHECK_DEPS_OUTDATED_LOCKFILE', `Setting ${outdatedLockfileSettingName} of lockfile in ${wantedLockfileDir} is outdated`, {
      hint: 'Run `pnpm install` to update the lockfile',
    })
  }

  if (!satisfiesPackageManifest(
    {
      autoInstallPeers,
      excludeLinksFromLockfile,
    },
    wantedLockfile.importers[projectId],
    projectManifest
  ).satisfies) {
    throw new PnpmError('RUN_CHECK_DEPS_UNSATISFIED_PKG_MANIFEST', `The lockfile in ${wantedLockfileDir} does not satisfy project of id ${projectId}`, {
      hint: 'Run `pnpm install` to update the lockfile',
    })
  }

  if (!await linkedPackagesAreUpToDate({
    linkWorkspacePackages: !!linkWorkspacePackages,
    lockfileDir: wantedLockfileDir,
    manifestsByDir: getManifestsByDir(),
    workspacePackages: getWorkspacePackages(),
    lockfilePackages: wantedLockfile.packages,
  }, {
    dir: projectDir,
    manifest: projectManifest,
    snapshot: wantedLockfile.importers[projectId],
  })) {
    throw new PnpmError('RUN_CHECK_DEPS_LINKED_PKGS_OUTDATED', `The linked packages by ${projectDir} is outdated`, {
      hint: 'Run `pnpm install` to update the packages',
    })
  }
}

/**
 * Returns the name of the first dependency declared with a local file
 * specifier in any of the given manifests, or `undefined` when there is none.
 * `link:` dependencies are excluded: they are symlinked, so changes inside
 * them flow through without a reinstall. Dependency groups excluded from the
 * current install (per `include`) are skipped: their local file dependencies
 * are not installed, so their contents cannot be stale. `catalog:` specs are
 * dereferenced through the catalogs config: the catalog resolver only bans
 * the `workspace:`, `link:`, and `file:` protocols, so a catalog entry can
 * still hold a bare local path (`../lib`, `vendor/pkg.tgz`) that resolves to
 * a local file dependency.
 */
function findLocalFileDep (manifests: ProjectManifest[], include?: IncludedDependencies, catalogs?: Catalogs): string | undefined {
  for (const manifest of manifests) {
    for (const depField of DEPENDENCIES_FIELDS) {
      if (include?.[depField] === false) continue
      const depName = findLocalFileDepInRecord(manifest[depField], catalogs)
      if (depName != null) return depName
    }
  }
  return undefined
}

/**
 * Returns the name of the first dependency in `deps` declared with (or
 * resolving through a catalog to) a local file specifier, or `undefined`.
 */
function findLocalFileDepInRecord (deps: Record<string, string> | undefined, catalogs?: Catalogs): string | undefined {
  if (deps == null) return undefined
  for (const [depName, spec] of Object.entries(deps)) {
    // A malformed manifest may carry a non-string spec; skip it rather
    // than throw — checkDepsStatus() must never crash.
    if (typeof spec !== 'string') continue
    if (isLocalFileSpec(spec)) return depName
    // Only catalog: specs consult the catalogs, so skip the lookup for
    // everything else to keep the optimistic fast path cheap.
    if (!spec.startsWith('catalog:')) continue
    const catalogResult = resolveFromCatalog(catalogs ?? {}, { alias: depName, bareSpecifier: spec })
    if (catalogResult.type === 'found' && isLocalFileSpec(catalogResult.resolution.specifier)) return depName
  }
  return undefined
}

/**
 * Returns the selector of the first `packageExtensions` entry that injects a
 * local file dependency, or `undefined` when there is none. Package
 * extensions are merged into matching packages' manifests by a read-package
 * hook during install, so a `file:`/local-path/tarball spec added there has
 * the same content-change blind spot as a direct local file dependency
 * without appearing in any project manifest. Only `dependencies` and
 * `optionalDependencies` are scanned: peer dependencies are resolved from the
 * graph rather than fetched, so a local spec there is never installed.
 */
function findLocalFilePackageExtension (packageExtensions: CheckDepsStatusOptions['packageExtensions'], include?: IncludedDependencies, catalogs?: Catalogs): string | undefined {
  if (packageExtensions == null) return undefined
  for (const [selector, extension] of Object.entries(packageExtensions)) {
    if (findLocalFileDepInRecord(extension.dependencies, catalogs) != null) return selector
    if (include?.optionalDependencies === false) continue
    if (findLocalFileDepInRecord(extension.optionalDependencies, catalogs) != null) return selector
  }
  return undefined
}

/**
 * Returns the selector of the first override that maps to a local file
 * specifier, or `undefined` when there is none. An override redirects every
 * matching dependency in the graph to its specifier, so a local file override
 * makes the installed contents depend on that directory or tarball the same
 * way a direct local file dependency does. Overrides are run through
 * `parseOverrides` so `catalog:` specs are dereferenced before the check.
 * `parseOverrides` throws on a misconfigured catalog or invalid selector;
 * that propagates to the outer catch in `checkDepsStatus`, which reports
 * not-up-to-date, and the resulting full install surfaces the same error.
 */
function findLocalFileOverride (overrides: Record<string, string> | undefined, catalogs?: Catalogs): string | undefined {
  if (overrides == null || isEmpty(overrides)) return undefined
  return parseOverrides(overrides, catalogs)
    .find(({ newBareSpecifier }) => isLocalFileSpec(newBareSpecifier))?.selector
}

const LOCAL_PATH_PREFIX = /^(?:[./\\]|~[/\\]|[a-z]:)/i
const LOCAL_TARBALL_EXTENSION = /\.(?:tgz|tar\.gz|tar)$/i

/**
 * Whether the specifier resolves to a local directory or tarball whose
 * contents can change without any manifest or lockfile mtime moving: the
 * `file:` protocol, path-prefixed specs (`./`, `../`, `~/`, absolute POSIX
 * paths, and Windows drive paths — including drive-relative ones like
 * `C:dir`, matching the local resolver's `isFilespec`), and bare tarball
 * file names.
 *
 * Deliberately narrower than the local resolver's bare-path matching: a bare
 * `dir/file.tgz`-less path like `user/repo` is statically indistinguishable
 * from a git shorthand at this layer, and matching it would disable the
 * repeat-install fast path for every project with git dependencies. Such
 * specs (and anything else carrying a protocol or URL) stay on the fast
 * path. `catalog:` specs also return false here — callers dereference them
 * through the catalogs config first, because a catalog entry may hold a
 * bare local path (the catalog resolver only bans the `workspace:`,
 * `link:`, and `file:` protocols).
 */
function isLocalFileSpec (spec: string): boolean {
  if (spec.startsWith('file:')) return true
  if (LOCAL_PATH_PREFIX.test(spec)) return true
  if (spec.includes(':')) return false
  // A `#` here means a hosted-git shorthand committish (`user/repo#release.tgz`),
  // not a local tarball — the `file:` and path-prefixed cases already returned above.
  if (spec.includes('#')) return false
  return LOCAL_TARBALL_EXTENSION.test(spec)
}

function throwLockfileNotFound (wantedLockfileDir: string): never {
  throw new PnpmError('RUN_CHECK_DEPS_LOCKFILE_NOT_FOUND', `Cannot find a lockfile in ${wantedLockfileDir}`, {
    hint: 'Run `pnpm install` to create the lockfile',
  })
}

/**
 * When `<lockfileDir>/pnpm-lock.yaml` is missing but the current lockfile
 * exists, returns the current lockfile so the caller can restore
 * `pnpm-lock.yaml` from it. `undefined` when the wanted lockfile is present
 * (nothing to restore) or when there is no current lockfile to restore from.
 */
async function missingWantedLockfileStandIn (lockfileDir: string, wantedLockfileName: string): Promise<CheckDepsStatusResult['wantedLockfileToRestore']> {
  if (safeStatSync(path.join(lockfileDir, wantedLockfileName)) != null) return undefined
  const currentLockfile = await readCurrentLockfile(path.join(lockfileDir, 'node_modules/.pnpm'), { ignoreIncompatible: false })
  if (currentLockfile == null) return undefined
  return { lockfile: currentLockfile, lockfileDir }
}

function getWantedLockfileDirs (opts: {
  allProjects: Project[] | undefined
  lockfileDir: string | undefined
  rootProjectManifestDir: string
  sharedWorkspaceLockfile: boolean | undefined
  workspaceDir: string | undefined
}): string[] {
  if (opts.allProjects && opts.workspaceDir && opts.sharedWorkspaceLockfile === false) {
    return [...new Set(opts.allProjects.map(({ rootDir }) => rootDir))]
  }
  return [opts.lockfileDir ?? opts.workspaceDir ?? opts.rootProjectManifestDir]
}

function scanWantedLockfiles (lockfileDirs: string[], lastValidatedTimestamp: number, opts: {
  wantedLockfileName: string
  mergeGitBranchLockfiles?: boolean
}): {
  conflictedDir: string | undefined
  anyModified: boolean
  anyMissing: boolean
} {
  let conflictedDir: string | undefined
  let anyModified = false
  let anyMissing = false
  for (const lockfileDir of lockfileDirs) {
    // With `mergeGitBranchLockfiles`, `readWantedLockfile` merges every
    // `pnpm-lock.*.yaml`, so a change in any of them changes the wanted
    // lockfile and must be detected here.
    const lockfileNames = opts.mergeGitBranchLockfiles
      ? gitBranchLockfileNames(lockfileDir, opts.wantedLockfileName)
      : [opts.wantedLockfileName]
    let foundInDir = false
    for (const lockfileName of lockfileNames) {
      let mtime: number
      try {
        mtime = fs.statSync(path.join(lockfileDir, lockfileName)).mtime.valueOf()
      } catch (err: unknown) {
        if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') continue
        throw err
      }
      foundInDir = true
      if (mtime <= lastValidatedTimestamp) continue
      anyModified = true
      if (wantedLockfileHasMergeConflictsSync(lockfileDir, lockfileName)) {
        conflictedDir = lockfileDir
        return { conflictedDir, anyModified, anyMissing }
      }
    }
    if (!foundInDir) anyMissing = true
  }
  return { conflictedDir, anyModified, anyMissing }
}

function gitBranchLockfileNames (lockfileDir: string, wantedLockfileName: string): string[] {
  let branchLockfileNames: string[]
  try {
    branchLockfileNames = getGitBranchLockfileNamesSync(lockfileDir)
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      branchLockfileNames = []
    } else {
      throw err
    }
  }
  return branchLockfileNames.includes(wantedLockfileName)
    ? branchLockfileNames
    : [wantedLockfileName, ...branchLockfileNames]
}

async function patchesOrHooksAreModified (opts: {
  patchedDependencies?: Record<string, string>
  rootDir: string
  lastValidatedTimestamp: number
  currentPnpmfiles: string[]
  previousPnpmfiles: string[]
}): Promise<string | undefined> {
  if (opts.patchedDependencies) {
    const allPatchStats = await Promise.all(Object.values(opts.patchedDependencies).map((patchFile) => {
      return safeStat(patchFile)
    }))
    if (allPatchStats.some(
      (patch) =>
        patch && patch.mtime.valueOf() > opts.lastValidatedTimestamp
    )) {
      return 'Patches were modified'
    }
  }
  if (!equals(opts.currentPnpmfiles, opts.previousPnpmfiles)) {
    return 'The list of pnpmfiles changed.'
  }
  for (const pnpmfilePath of opts.currentPnpmfiles) {
    const pnpmfileStats = safeStatSync(pnpmfilePath)
    if (pnpmfileStats == null) {
      return `pnpmfile at "${pnpmfilePath}" was removed`
    }
    if (pnpmfileStats.mtime.valueOf() > opts.lastValidatedTimestamp) {
      return `pnpmfile at "${pnpmfilePath}" was modified`
    }
  }
  return undefined
}
