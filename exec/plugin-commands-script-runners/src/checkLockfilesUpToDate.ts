import fs from 'fs'
import path from 'path'
import util from 'util'
import equals from 'ramda/src/equals'
// import isEmpty from 'ramda/src/isEmpty'
import { type Config, type OptionsFromRootManifest, getOptionsFromRootManifest } from '@pnpm/config'
import { MANIFEST_BASE_NAMES, WANTED_LOCKFILE } from '@pnpm/constants'
import { hashObjectNullableWithPrefix } from '@pnpm/crypto.object-hasher'
import { PnpmError } from '@pnpm/error'
import { type Lockfile, readCurrentLockfile, readWantedLockfile } from '@pnpm/lockfile.fs'
import {
  calcPatchHashes,
  createOverridesMapFromParsed,
  getOutdatedLockfileSetting,
} from '@pnpm/lockfile.settings-checker'
import { globalWarn } from '@pnpm/logger'
// TODO: check if list of dependencies are equal
// import { getAllDependenciesFromManifest } from '@pnpm/manifest-utils'
import { parseOverrides } from '@pnpm/parse-overrides'
import { loadPackagesList, updatePackagesList } from '@pnpm/workspace.packages-list-cache'

// The scripts that `pnpm run` executes are likely to also execute other `pnpm run`.
// We don't want this check (which can be quite expensive) to repeat.
// The solution is to use an env key to disable the check.
export const SKIP_ENV_KEY = 'pnpm_run_skip_deps_check'
export const DISABLE_DEPS_CHECK_ENV = {
  [SKIP_ENV_KEY]: 'true',
} as const satisfies Env

export interface Env extends NodeJS.ProcessEnv {
  [SKIP_ENV_KEY]?: string
}

export type ShouldRunCheckOptions = Pick<Config, 'checkDepsBeforeRunScripts'>

export const shouldRunCheck = (opts: ShouldRunCheckOptions, env: Env): boolean => !!opts.checkDepsBeforeRunScripts && !env[SKIP_ENV_KEY]

export type CheckLockfilesUpToDateOptions = Partial<Pick<Config,
| 'allProjects'
| 'autoInstallPeers'
| 'cacheDir'
| 'catalogs'
| 'excludeLinksFromLockfile'
| 'hooks'
| 'peersSuffixMaxLength'
| 'rootProjectManifest'
| 'rootProjectManifestDir'
| 'sharedWorkspaceLockfile'
| 'virtualStoreDir'
| 'workspaceDir'
>>

export async function checkLockfilesUpToDate (opts: CheckLockfilesUpToDateOptions): Promise<void> {
  const {
    allProjects,
    cacheDir,
    catalogs,
    rootProjectManifest,
    rootProjectManifestDir,
    sharedWorkspaceLockfile,
    virtualStoreDir,
    workspaceDir,
  } = opts

  if (!cacheDir || !virtualStoreDir) return

  const rootManifestOptions = rootProjectManifest && rootProjectManifestDir
    ? getOptionsFromRootManifest(rootProjectManifestDir, rootProjectManifest)
    : undefined

  if (allProjects && workspaceDir) {
    const packagesList = await loadPackagesList({ cacheDir, workspaceDir })
    if (!packagesList) {
      throw new PnpmError('RUN_CHECK_DEPS_NO_CACHE', 'Cannot check whether dependencies are outdated', {
        hint: 'Run `pnpm install` to create the cache',
      })
    }

    if (!equals(packagesList.catalogs ?? {}, catalogs ?? {})) {
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
      const attempts = await Promise.all(MANIFEST_BASE_NAMES.map(async manifestBaseName => {
        const manifestPath = path.join(project.rootDir, manifestBaseName)
        let manifestStats: fs.Stats
        try {
          manifestStats = await fs.promises.stat(manifestPath)
        } catch (error) {
          if (util.types.isNativeError(error) && 'code' in error && error.code === 'ENOENT') {
            return undefined
          }
          throw error
        }
        return manifestStats
      }))
      const manifestStats = attempts.find(x => !!x)
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

    if (modifiedProjects.length === 0) return

    let readWantedLockfileAndDir: (projectDir: string) => Promise<{
      wantedLockfile: Lockfile
      wantedLockfileDir: string
    }>
    if (sharedWorkspaceLockfile) {
      const wantedLockfileStats = await readStatsIfExists(path.join(workspaceDir, WANTED_LOCKFILE))
      if (!wantedLockfileStats) return throwLockfileNotFound(workspaceDir)

      const wantedLockfilePromise = readWantedLockfile(workspaceDir, { ignoreIncompatible: false })
      if (wantedLockfileStats.mtime.valueOf() > packagesList.lastValidatedTimestamp) {
        const virtualStoreDir = path.join(workspaceDir, 'node_modules', '.pnpm')
        const currentLockfile = await readCurrentLockfile(virtualStoreDir, { ignoreIncompatible: false })
        const wantedLockfile = (await wantedLockfilePromise) ?? throwLockfileNotFound(workspaceDir)
        assertLockfilesEqual(currentLockfile, wantedLockfile)
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
          readStatsIfExists(path.join(wantedLockfileDir, WANTED_LOCKFILE)),
        ])

        if (!wantedLockfileStats) return throwLockfileNotFound(wantedLockfileDir)
        if (wantedLockfileStats.mtime.valueOf() > packagesList.lastValidatedTimestamp) {
          const virtualStoreDir = path.join(wantedLockfileDir, 'node_modules', '.pnpm')
          const currentLockfile = await readCurrentLockfile(virtualStoreDir, { ignoreIncompatible: false })
          const wantedLockfile = (await wantedLockfilePromise) ?? throwLockfileNotFound(wantedLockfileDir)
          assertLockfilesEqual(currentLockfile, wantedLockfile)
        }

        return {
          wantedLockfile: (await wantedLockfilePromise) ?? throwLockfileNotFound(wantedLockfileDir),
          wantedLockfileDir,
        }
      }
    }

    await Promise.all(modifiedProjects.map(async ({ project }) => {
      const { wantedLockfile, wantedLockfileDir } = await readWantedLockfileAndDir(project.rootDir)

      await handleSingleProject({
        config: opts,
        rootDir: workspaceDir,
        rootManifestOptions,
        wantedLockfile,
        wantedLockfileDir,
      })
    }))

    // update lastValidatedTimestamp to prevent pointless repeat
    await updatePackagesList({
      allProjects,
      cacheDir,
      lastValidatedTimestamp: Date.now(),
      workspaceDir,
    })
  } else if (rootProjectManifest && rootProjectManifestDir) {
    const virtualStoreDir = path.join(rootProjectManifestDir, 'node_modules', '.pnpm')
    const currentLockfilePromise = readCurrentLockfile(virtualStoreDir, { ignoreIncompatible: false })
    const wantedLockfilePromise = readWantedLockfile(rootProjectManifestDir, { ignoreIncompatible: false })
    const [
      currentLockfileStats,
      wantedLockfileStats,
    ] = await Promise.all([
      readStatsIfExists(path.join(virtualStoreDir, 'lock.yaml')),
      readStatsIfExists(path.join(rootProjectManifestDir, WANTED_LOCKFILE)),
    ])

    if (!wantedLockfileStats) return throwLockfileNotFound(rootProjectManifestDir)

    if (currentLockfileStats && wantedLockfileStats.mtime.valueOf() > currentLockfileStats.mtime.valueOf()) {
      const currentLockfile = await currentLockfilePromise
      const wantedLockfile = (await wantedLockfilePromise) ?? throwLockfileNotFound(rootProjectManifestDir)
      assertLockfilesEqual(currentLockfile, wantedLockfile)
    }

    await handleSingleProject({
      config: opts,
      rootDir: rootProjectManifestDir,
      rootManifestOptions,
      wantedLockfile: (await wantedLockfilePromise) ?? throwLockfileNotFound(rootProjectManifestDir),
      wantedLockfileDir: rootProjectManifestDir,
    })
  } else {
    globalWarn('Impossible variant detected! Skipping check.')
  }
}

interface HandleSingleProjectOptions {
  config: CheckLockfilesUpToDateOptions
  rootDir: string
  rootManifestOptions: OptionsFromRootManifest | undefined
  wantedLockfile: Lockfile
  wantedLockfileDir: string
}

async function handleSingleProject (opts: HandleSingleProjectOptions): Promise<void> {
  const {
    config,
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
    throw new PnpmError('RUN_CHECK_DEPS_OUTDATED_LOCKFILE', `The lockfile in ${wantedLockfileDir} contains outdated information`, {
      hint: 'Run `pnpm install` to update the lockfile',
    })
  }
}

async function readStatsIfExists (filePath: string): Promise<fs.Stats | undefined> {
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

function assertLockfilesEqual (currentLockfile: Lockfile | null, wantedLockfile: Lockfile): void {
  if (!currentLockfile) {
    // make sure that no importer of wantedLockfile has any dependency
    for (const [name, snapshot] of Object.entries(wantedLockfile.importers)) {
      if (!equals(snapshot.specifiers, {})) {
        throw new PnpmError('RUN_CHECK_DEPS_NO_DEPS', `Project ${name} requires dependencies but none was installed.`, {
          hint: 'Run `pnpm install` to install dependencies',
        })
      }
    }
  } else if (!equals(currentLockfile, wantedLockfile)) {
    // TODO: add wantedLockfileDir
    throw new PnpmError('RUN_CHECK_DEPS_OUTDATED_DEPS', 'The installed dependencies in the modules directory is not up-to-date with the lockfile.', {
      hint: 'Run `pnpm install` to update dependencies.',
    })
  }
}

function throwLockfileNotFound (wantedLockfileDir: string): never {
  throw new PnpmError('RUN_CHECK_DEPS_LOCKFILE_NOT_FOUND', `Cannot find a lockfile in ${wantedLockfileDir}`, {
    hint: 'Run `pnpm install` to create the lockfile',
  })
}
