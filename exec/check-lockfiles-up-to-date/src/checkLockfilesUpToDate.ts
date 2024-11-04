import fs from 'fs'
import path from 'path'
import util from 'util'
import equals from 'ramda/src/equals'
import filter from 'ramda/src/filter'
import once from 'ramda/src/once'
import { type Config, type OptionsFromRootManifest, getOptionsFromRootManifest } from '@pnpm/config'
import { MANIFEST_BASE_NAMES, WANTED_LOCKFILE } from '@pnpm/constants'
import { hashObjectNullableWithPrefix } from '@pnpm/crypto.object-hasher'
import { PnpmError } from '@pnpm/error'
import {
  type Lockfile,
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
import { arrayOfWorkspacePackagesToMap } from '@pnpm/get-context'
import { parseOverrides } from '@pnpm/parse-overrides'
import { type WorkspacePackages } from '@pnpm/resolver-base'
import {
  type DependencyManifest,
  type Project,
  type ProjectId,
  type ProjectManifest,
} from '@pnpm/types'
import { findWorkspacePackages } from '@pnpm/workspace.find-packages'
import { loadPackagesList, updatePackagesList } from '@pnpm/workspace.packages-list-cache'
import { readWorkspaceManifest } from '@pnpm/workspace.read-manifest'
import { assertLockfilesEqual } from './assertLockfilesEqual'
import { statManifestFile } from './statManifestFile'

export type CheckLockfilesUpToDateOptions = Pick<Config,
| 'allProjects'
| 'autoInstallPeers'
| 'catalogs'
| 'excludeLinksFromLockfile'
| 'linkWorkspacePackages'
| 'hooks'
| 'peersSuffixMaxLength'
| 'rootProjectManifest'
| 'rootProjectManifestDir'
| 'sharedWorkspaceLockfile'
| 'workspaceDir'
>

export async function checkLockfilesUpToDate (opts: CheckLockfilesUpToDateOptions): Promise<void> {
  const {
    allProjects,
    autoInstallPeers,
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

  if (allProjects && workspaceDir) {
    const packagesList = await loadPackagesList(workspaceDir)
    if (!packagesList) {
      throw new PnpmError('RUN_CHECK_DEPS_NO_CACHE', 'Cannot check whether dependencies are outdated', {
        hint: 'Run `pnpm install` to create the cache',
      })
    }

    if (!equals(
      filter(value => value != null, packagesList.catalogs ?? {}),
      filter(value => value != null, catalogs ?? {})
    )) {
      throw new PnpmError('RUN_CHECK_DEPS_OUTDATED', 'Catalogs cache outdated', {
        hint: 'Run `pnpm install` to update the catalogs cache',
      })
    }

    const currentProjectRootDirs = allProjects.map(project => project.rootDir).sort()
    if (!equals(packagesList.projectRootDirs, currentProjectRootDirs)) {
      throw new PnpmError('RUN_CHECK_DEPS_WORKSPACE_STRUCTURE_CHANGED', 'The workspace structure has changed since last install', {
        hint: 'Run `pnpm install` to update the workspace structure and dependencies tree',
      })
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
        manifestStats.mtime.valueOf() > packagesList.lastValidatedTimestamp
    )

    if (modifiedProjects.length === 0) {
      logger.debug({ msg: 'No manifest files are modified after the last validation. Exiting check.' })
      return
    }

    logger.debug({ msg: 'Some manifest files are modified after the last validation. Continuing check.' })

    let readWantedLockfileAndDir: (projectDir: string) => Promise<{
      wantedLockfile: Lockfile
      wantedLockfileDir: string
    }>
    if (sharedWorkspaceLockfile) {
      const wantedLockfileStats = await statIfExists(path.join(workspaceDir, WANTED_LOCKFILE))
      if (!wantedLockfileStats) return throwLockfileNotFound(workspaceDir)

      const wantedLockfilePromise = readWantedLockfile(workspaceDir, { ignoreIncompatible: false })
      if (wantedLockfileStats.mtime.valueOf() > packagesList.lastValidatedTimestamp) {
        const virtualStoreDir = path.join(workspaceDir, 'node_modules', '.pnpm')
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
        const [
          wantedLockfileStats,
        ] = await Promise.all([
          statIfExists(path.join(wantedLockfileDir, WANTED_LOCKFILE)),
        ])

        if (!wantedLockfileStats) return throwLockfileNotFound(wantedLockfileDir)
        if (wantedLockfileStats.mtime.valueOf() > packagesList.lastValidatedTimestamp) {
          const virtualStoreDir = path.join(wantedLockfileDir, 'node_modules', '.pnpm')
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

    await Promise.all(modifiedProjects.map(async ({ project }) => {
      const { wantedLockfile, wantedLockfileDir } = await readWantedLockfileAndDir(project.rootDir)

      await assertWantedLockfileUpToDate({
        autoInstallPeers,
        config: opts,
        excludeLinksFromLockfile,
        linkWorkspacePackages,
        getManifestsByDir,
        getWorkspacePackages,
        projectDir: project.rootDir,
        projectId: getProjectId(project),
        projectManifest: project.manifest,
        rootDir: workspaceDir,
        rootManifestOptions,
        wantedLockfile,
        wantedLockfileDir,
      })
    }))

    // update lastValidatedTimestamp to prevent pointless repeat
    await updatePackagesList({
      allProjects,
      lastValidatedTimestamp: Date.now(),
      workspaceDir,
    })

    return
  }

  if (!allProjects) {
    const workspaceRoot = workspaceDir ?? rootProjectManifestDir
    const workspaceManifest = await readWorkspaceManifest(workspaceRoot)
    if (workspaceManifest ?? workspaceDir) {
      const allProjects = await findWorkspacePackages(rootProjectManifestDir, {
        patterns: workspaceManifest?.packages,
        sharedWorkspaceLockfile,
      })
      return checkLockfilesUpToDate({
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
      await assertWantedLockfileUpToDate({
        autoInstallPeers,
        config: opts,
        excludeLinksFromLockfile,
        linkWorkspacePackages,
        getManifestsByDir: () => ({}),
        getWorkspacePackages: () => undefined,
        projectDir: rootProjectManifestDir,
        projectId: '.' as ProjectId,
        projectManifest: rootProjectManifest,
        rootDir: rootProjectManifestDir,
        rootManifestOptions,
        wantedLockfile: (await wantedLockfilePromise) ?? throwLockfileNotFound(rootProjectManifestDir),
        wantedLockfileDir: rootProjectManifestDir,
      })
    } else {
      logger.debug({ msg: 'The manifest file not newer than the lockfile. Exiting check.' })
    }

    return
  }

  // `opts.allProject` being `undefined` means that the run command was not run with `--recursive`.
  // `rootProjectManifest` being `undefined` means that there's no root manifest.
  // Both means that `pnpm run` would fail, so checking lockfiles here is pointless.
  globalWarn('Skipping check.')
}

interface AssertWantedLockfileUpToDateOptions {
  autoInstallPeers?: boolean
  config: CheckLockfilesUpToDateOptions
  excludeLinksFromLockfile?: boolean
  linkWorkspacePackages: boolean | 'deep'
  getManifestsByDir: () => Record<string, DependencyManifest>
  getWorkspacePackages: () => WorkspacePackages | undefined
  projectDir: string
  projectId: ProjectId
  projectManifest: ProjectManifest
  rootDir: string
  rootManifestOptions: OptionsFromRootManifest | undefined
  wantedLockfile: Lockfile
  wantedLockfileDir: string
}

async function assertWantedLockfileUpToDate (opts: AssertWantedLockfileUpToDateOptions): Promise<void> {
  const {
    autoInstallPeers,
    config,
    excludeLinksFromLockfile,
    linkWorkspacePackages,
    getManifestsByDir,
    getWorkspacePackages,
    projectDir,
    projectId,
    projectManifest,
    rootDir,
    rootManifestOptions,
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
