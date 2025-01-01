import fs from 'fs'
import path from 'path'
import util from 'util'
import equals from 'ramda/src/equals'
import isEmpty from 'ramda/src/isEmpty'
import filter from 'ramda/src/filter'
import once from 'ramda/src/once'
import { type Config, type OptionsFromRootManifest, getOptionsFromRootManifest } from '@pnpm/config'
import { MANIFEST_BASE_NAMES, WANTED_LOCKFILE } from '@pnpm/constants'
import { hashObjectNullableWithPrefix } from '@pnpm/crypto.object-hasher'
import { PnpmError } from '@pnpm/error'
import { arrayOfWorkspacePackagesToMap } from '@pnpm/get-context'
import {
  type LockfileObject,
  getLockfileImporterId,
  readCurrentLockfile,
  readWantedLockfile,
} from '@pnpm/lockfile.fs'
import {
  calcPatchHashes,
  createOverridesMapFromParsed,
  getOutdatedLockfileSetting,
} from '@pnpm/lockfile.settings-checker'
import {
  linkedPackagesAreUpToDate,
  getWorkspacePackagesByDirectory,
  satisfiesPackageManifest,
} from '@pnpm/lockfile.verification'
import { globalWarn, logger } from '@pnpm/logger'
import { parseOverrides } from '@pnpm/parse-overrides'
import { getPnpmfilePath } from '@pnpm/pnpmfile'
import { type WorkspacePackages } from '@pnpm/resolver-base'
import {
  type DependencyManifest,
  type Project,
  type ProjectId,
  type ProjectManifest,
} from '@pnpm/types'
import { findWorkspacePackages } from '@pnpm/workspace.find-packages'
import { readWorkspaceManifest } from '@pnpm/workspace.read-manifest'
import { loadWorkspaceState, updateWorkspaceState } from '@pnpm/workspace.state'
import { assertLockfilesEqual } from './assertLockfilesEqual'
import { safeStat, safeStatSync, statManifestFile } from './statManifestFile'
import { type WorkspaceStateSettings } from '@pnpm/workspace.state/src/types'

export type CheckDepsStatusOptions = Pick<Config,
| 'allProjects'
| 'autoInstallPeers'
| 'catalogs'
| 'excludeLinksFromLockfile'
| 'injectWorkspacePackages'
| 'linkWorkspacePackages'
| 'hooks'
| 'peersSuffixMaxLength'
| 'rootProjectManifest'
| 'rootProjectManifestDir'
| 'sharedWorkspaceLockfile'
| 'virtualStoreDir'
| 'workspaceDir'
| 'patchesDir'
| 'pnpmfile'
> & {
  ignoreFilteredInstallCache?: boolean
} & WorkspaceStateSettings

export async function checkDepsStatus (opts: CheckDepsStatusOptions): Promise<{ upToDate: boolean | undefined, issue?: string }> {
  try {
    return await _checkDepsStatus(opts)
  } catch (error) {
    if (util.types.isNativeError(error) && 'code' in error && String(error.code).startsWith('ERR_PNPM_RUN_CHECK_DEPS_')) {
      return {
        upToDate: false,
        issue: error.message,
      }
    }
    // This function never throws an error.
    // We want to ensure that pnpm CLI never crashes when checking the status of dependencies.
    // In the worst-case scenario, the install will run redundantly.
    return {
      upToDate: undefined,
      issue: util.types.isNativeError(error) ? error.message : undefined,
    }
  }
}

async function _checkDepsStatus (opts: CheckDepsStatusOptions): Promise<{ upToDate: boolean | undefined, issue?: string }> {
  const {
    allProjects,
    autoInstallPeers,
    injectWorkspacePackages,
    catalogs,
    excludeLinksFromLockfile,
    linkWorkspacePackages,
    rootProjectManifest,
    rootProjectManifestDir,
    sharedWorkspaceLockfile,
    workspaceDir,
  } = opts

  const rootManifestOptions = rootProjectManifest && rootProjectManifestDir
    ? getOptionsFromRootManifest(rootProjectManifestDir, rootProjectManifest)
    : undefined

  const workspaceState = loadWorkspaceState(workspaceDir ?? rootProjectManifestDir)
  if (!workspaceState) {
    return {
      upToDate: false,
      issue: 'Cannot check whether dependencies are outdated',
    }
  }
  if (opts.ignoreFilteredInstallCache && workspaceState.filteredInstall) {
    return { upToDate: undefined }
  }

  if (workspaceState.settings) {
    for (const [settingName, settingValue] of Object.entries(workspaceState.settings)) {
      if (settingName === 'catalogs') continue
      // @ts-expect-error
      if (!equals(settingValue, opts[settingName])) {
        return {
          upToDate: false,
          issue: `The value of the ${settingName} setting has changed`,
        }
      }
    }
  }
  if ((rootProjectManifest?.pnpm?.configDependencies != null || workspaceState.configDependencies != null) && !equals(rootProjectManifest?.pnpm?.configDependencies ?? {}, workspaceState.configDependencies ?? {})) {
    return {
      upToDate: false,
      issue: 'Configuration dependencies are not up to date',
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
      }
    }

    const allManifestStats = await Promise.all(allProjects.map(async project => {
      const manifestStats = await statManifestFile(project.rootDir)
      if (!manifestStats) {
        // this error should not happen
        throw new Error(`Cannot find one of ${MANIFEST_BASE_NAMES.join(', ')} in ${project.rootDir}`)
      }
      return { project, manifestStats }
    }))

    const modifiedProjects = allManifestStats.filter(
      ({ manifestStats }) =>
        manifestStats.mtime.valueOf() > workspaceState.lastValidatedTimestamp
    )

    if (modifiedProjects.length === 0) {
      logger.debug({ msg: 'No manifest files were modified since the last validation. Exiting check.' })
      return { upToDate: true }
    }

    const issue = await patchesAreModified({
      rootManifestOptions,
      rootDir: rootProjectManifestDir,
      lastValidatedTimestamp: workspaceState.lastValidatedTimestamp,
      pnpmfile: opts.pnpmfile,
      hadPnpmfile: workspaceState.pnpmfileExists,
    })
    if (issue) {
      return { upToDate: false, issue }
    }

    logger.debug({ msg: 'Some manifest files were modified since the last validation. Continuing check.' })

    let readWantedLockfileAndDir: (projectDir: string) => Promise<{
      wantedLockfile: LockfileObject
      wantedLockfileDir: string
    }>
    if (sharedWorkspaceLockfile) {
      let wantedLockfileStats: fs.Stats
      try {
        wantedLockfileStats = fs.statSync(path.join(workspaceDir, WANTED_LOCKFILE))
      } catch (error) {
        if (util.types.isNativeError(error) && 'code' in error && error.code === 'ENOENT') {
          return throwLockfileNotFound(workspaceDir)
        } else {
          throw error
        }
      }

      const wantedLockfilePromise = readWantedLockfile(workspaceDir, { ignoreIncompatible: false })
      if (wantedLockfileStats.mtime.valueOf() > workspaceState.lastValidatedTimestamp) {
        const virtualStoreDir = opts.virtualStoreDir ?? path.join(workspaceDir, 'node_modules', '.pnpm')
        const currentLockfile = await readCurrentLockfile(virtualStoreDir, { ignoreIncompatible: false })
        const wantedLockfile = (await wantedLockfilePromise) ?? throwLockfileNotFound(workspaceDir)
        assertLockfilesEqual(currentLockfile, wantedLockfile, workspaceDir)
      }
      readWantedLockfileAndDir = async () => ({
        wantedLockfile: (await wantedLockfilePromise) ?? throwLockfileNotFound(workspaceDir),
        wantedLockfileDir: workspaceDir,
      })
    } else {
      readWantedLockfileAndDir = async wantedLockfileDir => {
        const wantedLockfilePromise = readWantedLockfile(wantedLockfileDir, { ignoreIncompatible: false })
        const wantedLockfileStats = await statIfExists(path.join(wantedLockfileDir, WANTED_LOCKFILE))

        if (!wantedLockfileStats) return throwLockfileNotFound(wantedLockfileDir)
        if (wantedLockfileStats.mtime.valueOf() > workspaceState.lastValidatedTimestamp) {
          const virtualStoreDir = opts.virtualStoreDir ?? path.join(wantedLockfileDir, 'node_modules', '.pnpm')
          const currentLockfile = await readCurrentLockfile(virtualStoreDir, { ignoreIncompatible: false })
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
      rootManifestOptions,
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
      return { upToDate: false, issue: (util.types.isNativeError(err) && 'message' in err) ? err.message : undefined }
    }

    // update lastValidatedTimestamp to prevent pointless repeat
    await updateWorkspaceState({
      allProjects,
      workspaceDir,
      pnpmfileExists: workspaceState.pnpmfileExists,
      settings: opts,
      filteredInstall: workspaceState.filteredInstall,
    })

    return { upToDate: true }
  }

  if (!allProjects) {
    const workspaceRoot = workspaceDir ?? rootProjectManifestDir
    const workspaceManifest = await readWorkspaceManifest(workspaceRoot)
    if (workspaceManifest ?? workspaceDir) {
      const allProjects = await findWorkspacePackages(rootProjectManifestDir, {
        patterns: workspaceManifest?.packages,
        sharedWorkspaceLockfile,
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
    const virtualStoreDir = path.join(rootProjectManifestDir, 'node_modules', '.pnpm')
    const currentLockfilePromise = readCurrentLockfile(virtualStoreDir, { ignoreIncompatible: false })
    const wantedLockfilePromise = readWantedLockfile(rootProjectManifestDir, { ignoreIncompatible: false })
    const [
      currentLockfileStats,
      wantedLockfileStats,
      manifestStats,
    ] = await Promise.all([
      statIfExists(path.join(virtualStoreDir, 'lock.yaml')),
      statIfExists(path.join(rootProjectManifestDir, WANTED_LOCKFILE)),
      statManifestFile(rootProjectManifestDir),
    ])

    if (!wantedLockfileStats) return throwLockfileNotFound(rootProjectManifestDir)

    const issue = await patchesAreModified({
      rootManifestOptions,
      rootDir: rootProjectManifestDir,
      lastValidatedTimestamp: wantedLockfileStats.mtime.valueOf(),
      pnpmfile: opts.pnpmfile,
      hadPnpmfile: workspaceState.pnpmfileExists,
    })
    if (issue) {
      return { upToDate: false, issue }
    }

    if (currentLockfileStats && wantedLockfileStats.mtime.valueOf() > currentLockfileStats.mtime.valueOf()) {
      const currentLockfile = await currentLockfilePromise
      const wantedLockfile = (await wantedLockfilePromise) ?? throwLockfileNotFound(rootProjectManifestDir)
      assertLockfilesEqual(currentLockfile, wantedLockfile, rootProjectManifestDir)
    }

    if (!manifestStats) {
      // this error should not happen
      throw new Error(`Cannot find one of ${MANIFEST_BASE_NAMES.join(', ')} in ${rootProjectManifestDir}`)
    }

    if (manifestStats.mtime.valueOf() > wantedLockfileStats.mtime.valueOf()) {
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
          rootManifestOptions,
        }, {
          projectDir: rootProjectManifestDir,
          projectId: '.' as ProjectId,
          projectManifest: rootProjectManifest,
          wantedLockfile: (await wantedLockfilePromise) ?? throwLockfileNotFound(rootProjectManifestDir),
          wantedLockfileDir: rootProjectManifestDir,
        })
      } catch (err) {
        return { upToDate: false, issue: (util.types.isNativeError(err) && 'message' in err) ? err.message : undefined }
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

    return { upToDate: true }
  }

  // `opts.allProject` being `undefined` means that the run command was not run with `--recursive`.
  // `rootProjectManifest` being `undefined` means that there's no root manifest.
  // Both means that `pnpm run` would fail, so checking lockfiles here is pointless.
  globalWarn('Skipping check.')
  return { upToDate: undefined }
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
  rootManifestOptions: OptionsFromRootManifest | undefined
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
    rootDir,
    rootManifestOptions,
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
    calcPatchHashes(rootManifestOptions?.patchedDependencies ?? {}, rootDir),
    config.hooks?.calculatePnpmfileChecksum?.(),
  ])

  const outdatedLockfileSettingName = getOutdatedLockfileSetting(wantedLockfile, {
    autoInstallPeers: config.autoInstallPeers,
    injectWorkspacePackages: config.injectWorkspacePackages,
    excludeLinksFromLockfile: config.excludeLinksFromLockfile,
    peersSuffixMaxLength: config.peersSuffixMaxLength,
    overrides: createOverridesMapFromParsed(parseOverrides(rootManifestOptions?.overrides ?? {}, config.catalogs)),
    ignoredOptionalDependencies: rootManifestOptions?.ignoredOptionalDependencies?.sort(),
    packageExtensionsChecksum: hashObjectNullableWithPrefix(rootManifestOptions?.packageExtensions),
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

async function statIfExists (filePath: string): Promise<fs.Stats | undefined> {
  let stats: fs.Stats
  try {
    stats = await fs.promises.stat(filePath)
  } catch (error) {
    if (util.types.isNativeError(error) && 'code' in error && error.code === 'ENOENT') {
      return undefined
    }
    throw error
  }
  return stats
}

function throwLockfileNotFound (wantedLockfileDir: string): never {
  throw new PnpmError('RUN_CHECK_DEPS_LOCKFILE_NOT_FOUND', `Cannot find a lockfile in ${wantedLockfileDir}`, {
    hint: 'Run `pnpm install` to create the lockfile',
  })
}

async function patchesAreModified (opts: {
  rootManifestOptions: OptionsFromRootManifest | undefined
  rootDir: string
  lastValidatedTimestamp: number
  pnpmfile: string
  hadPnpmfile: boolean
}): Promise<string | undefined> {
  if (opts.rootManifestOptions?.patchedDependencies) {
    const allPatchStats = await Promise.all(Object.values(opts.rootManifestOptions.patchedDependencies).map((patchFile) => {
      return safeStat(path.relative(opts.rootDir, patchFile))
    }))
    if (allPatchStats.some(
      (patch) =>
        patch && patch.mtime.valueOf() > opts.lastValidatedTimestamp
    )) {
      return 'Patches were modified'
    }
  }
  const pnpmfilePath = getPnpmfilePath(opts.rootDir, opts.pnpmfile)
  const pnpmfileStats = safeStatSync(pnpmfilePath)
  if (pnpmfileStats != null && pnpmfileStats.mtime.valueOf() > opts.lastValidatedTimestamp) {
    return `pnpmfile at "${pnpmfilePath}" was modified`
  }
  if (opts.hadPnpmfile && pnpmfileStats == null) {
    return `pnpmfile at "${pnpmfilePath}" was removed`
  }
  if (!opts.hadPnpmfile && pnpmfileStats != null) {
    return `pnpmfile at "${pnpmfilePath}" was added`
  }
  return undefined
}
