import fs from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import { parseOverrides } from '@pnpm/config.parse-overrides'
import type { Config, ConfigContext } from '@pnpm/config.reader'
import { MANIFEST_BASE_NAMES, WANTED_LOCKFILE } from '@pnpm/constants'
import { hashObjectNullableWithPrefix } from '@pnpm/crypto.object-hasher'
import { PnpmError } from '@pnpm/error'
import { arrayOfWorkspacePackagesToMap } from '@pnpm/installing.context'
import {
  getLockfileImporterId,
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
   * a `file:` dependency's directory (or a repacked `file:` tarball) go
   * unnoticed. Callers that skip the install entirely when this check reports
   * up-to-date must set this so that projects with `file:` dependencies
   * always run a real install, which refetches those dependencies
   * (https://github.com/pnpm/pnpm/issues/11795).
   */
  treatLocalFileDepsAsOutdated?: boolean
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

  if (nodeLinker === 'pnp') {
    globalWarn('verify-deps-before-run does not work with node-linker=pnp')
    return { upToDate: true, workspaceState: undefined }
  }

  if (opts.ignoreFilteredInstallCache && workspaceState.filteredInstall) {
    return { upToDate: undefined, workspaceState }
  }

  if (opts.treatLocalFileDepsAsOutdated) {
    const manifests = allProjects?.map(({ manifest }) => manifest) ??
      (rootProjectManifest ? [rootProjectManifest] : [])
    const localFileDep = findLocalFileDep(manifests)
    if (localFileDep != null) {
      return {
        upToDate: false,
        issue: `The dependency "${localFileDep}" uses the file: protocol and its contents may have changed`,
        workspaceState,
      }
    }
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

  const conflictedLockfileDir = findConflictedLockfileDir(getWantedLockfileDirs({
    allProjects,
    lockfileDir,
    rootProjectManifestDir,
    sharedWorkspaceLockfile,
    workspaceDir,
  }), workspaceState.lastValidatedTimestamp)
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

    if (modifiedProjects.length === 0) {
      logger.debug({ msg: 'No manifest files were modified since the last validation. Exiting check.' })
      const wantedLockfileToRestore = sharedWorkspaceLockfile && !opts.useGitBranchLockfile
        ? await missingWantedLockfileStandIn(workspaceDir)
        : undefined
      return { upToDate: true, workspaceState, wantedLockfileToRestore }
    }

    logger.debug({ msg: 'Some manifest files were modified since the last validation. Continuing check.' })

    let wantedLockfileToRestore: CheckDepsStatusResult['wantedLockfileToRestore']
    let readWantedLockfileAndDir: (projectDir: string) => Promise<{
      wantedLockfile: LockfileObject
      wantedLockfileDir: string
    }>
    if (sharedWorkspaceLockfile) {
      let wantedLockfileStats: fs.Stats | undefined
      try {
        wantedLockfileStats = fs.statSync(path.join(workspaceDir, WANTED_LOCKFILE))
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
        const wantedLockfilePromise = readWantedLockfile(workspaceDir, { ignoreIncompatible: false })
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
        const wantedLockfilePromise = readWantedLockfile(wantedLockfileDir, { ignoreIncompatible: false })
        const wantedLockfileStats = await safeStat(path.join(wantedLockfileDir, WANTED_LOCKFILE))

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
      await Promise.all(modifiedProjects.map(async ({ project }) => {
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
    const wantedLockfilePromise = readWantedLockfile(rootProjectManifestDir, { ignoreIncompatible: false })
    const [
      currentLockfileStats,
      wantedLockfileStats,
      manifestStats,
    ] = await Promise.all([
      safeStat(path.join(internalPnpmDir, 'lock.yaml')),
      safeStat(path.join(rootProjectManifestDir, WANTED_LOCKFILE)),
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
 * Returns the name of the first dependency declared with a `file:` specifier
 * in any of the given manifests, or `undefined` when there is none. `link:`
 * dependencies are excluded: they are symlinked, so changes inside them flow
 * through without a reinstall.
 */
function findLocalFileDep (manifests: ProjectManifest[]): string | undefined {
  for (const manifest of manifests) {
    for (const depField of DEPENDENCIES_FIELDS) {
      const deps = manifest[depField]
      if (deps == null) continue
      for (const [depName, spec] of Object.entries(deps)) {
        if (spec.startsWith('file:')) return depName
      }
    }
  }
  return undefined
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
async function missingWantedLockfileStandIn (lockfileDir: string): Promise<CheckDepsStatusResult['wantedLockfileToRestore']> {
  if (safeStatSync(path.join(lockfileDir, WANTED_LOCKFILE)) != null) return undefined
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

function findConflictedLockfileDir (lockfileDirs: string[], lastValidatedTimestamp: number): string | undefined {
  for (const lockfileDir of lockfileDirs) {
    let mtime: number
    try {
      mtime = fs.statSync(path.join(lockfileDir, WANTED_LOCKFILE)).mtime.valueOf()
    } catch (err: unknown) {
      if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') continue
      throw err
    }
    // If the lockfile hasn't been modified since the last successful install, it can't have
    // grown conflict markers — skip the read to preserve the optimistic fast-path.
    if (mtime <= lastValidatedTimestamp) continue
    if (wantedLockfileHasMergeConflictsSync(lockfileDir)) return lockfileDir
  }
  return undefined
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
