import assert from 'node:assert'
import util from 'node:util'

import { type BuildOptions, buildProjects as rebuildAll, buildSelectedPkgs } from '@pnpm/building.after-install'
import {
  type RecursiveSummary,
  throwOnCommandFail,
} from '@pnpm/cli.utils'
import {
  type Config,
  type ConfigContext,
  createProjectConfigRecord,
  getWorkspaceConcurrency,
} from '@pnpm/config.reader'
import { logger } from '@pnpm/logger'
import { createStoreController, type CreateStoreControllerOptions } from '@pnpm/store.connection-manager'
import type { Project, ProjectRootDir } from '@pnpm/types'
import { filteredProjectsDependencies } from '@pnpm/workspace.projects-sorter'
import { scheduleGraph, type TaskCompletion } from '@pnpm/workspace.task-scheduler'

type RecursiveRebuildOpts = CreateStoreControllerOptions & Pick<Config,
| 'enableGlobalVirtualStore'
| 'hoistPattern'
| 'ignorePnpmfile'
| 'ignoreScripts'
| 'lockfileDir'
| 'lockfileOnly'
| 'nodeLinker'
| 'packageConfigs'
| 'registriesByScope'
| 'sharedWorkspaceLockfile'
> & Pick<ConfigContext,
| 'hooks'
| 'rootProjectManifest'
| 'rootProjectManifestDir'
> & {
  pending?: boolean
} & Partial<Pick<Config, 'bail' | 'sort' | 'workspaceConcurrency'>>

export async function recursiveRebuild (
  allProjects: Project[],
  params: string[],
  opts: RecursiveRebuildOpts & {
    ignoredPackages?: Set<string>
  } & Required<Pick<ConfigContext, 'selectedProjectsGraph'>> & Pick<ConfigContext, 'allProjectsGraph' | 'prodAllProjectsGraph' | 'prodOnlySelectedProjectDirs'> & Required<Pick<Config, 'workspaceDir'>>
): Promise<void> {
  if (allProjects.length === 0) {
    // It might make sense to throw an exception in this case
    return
  }

  const pkgs = Object.values(opts.selectedProjectsGraph).map((wsPkg) => wsPkg.package)

  if (pkgs.length === 0) {
    return
  }
  const manifestsByPath: { [dir: string]: Omit<Project, 'rootDir' | 'rootDirRealPath'> } = {}
  for (const { rootDir, manifest, writeProjectManifest } of pkgs) {
    manifestsByPath[rootDir] = { manifest, writeProjectManifest }
  }

  const throwOnFail = throwOnCommandFail.bind(null, 'pnpm recursive rebuild')

  const projectDependencies = opts.sort !== false
    ? filteredProjectsDependencies(opts)
    : new Map((Object.keys(opts.selectedProjectsGraph).sort() as ProjectRootDir[]).map((rootDir) => [rootDir, []]))

  const store = await createStoreController(opts)

  const rebuildOpts = Object.assign(opts, {
    ownLifecycleHooksStdio: 'pipe',
    pruneLockfileImporters: ((opts.ignoredPackages == null) || opts.ignoredPackages.size === 0) &&
      pkgs.length === allProjects.length,
    storeController: store.ctrl,
    storeDir: store.dir,
    projectDependencies,
  }) as BuildOptions

  const result: RecursiveSummary = {}

  const projectConfigRecord = createProjectConfigRecord(opts) ?? {}

  async function getImporters () {
    return [...projectDependencies.keys()]
      .filter((rootDir) => !opts.ignoredPackages?.has(rootDir))
      .map((rootDir) => ({
        buildIndex: 0,
        manifest: manifestsByPath[rootDir].manifest,
        rootDir,
      }))
  }

  const rebuild = (
    params.length === 0
      ? rebuildAll
    : (importers: any, opts: any) => buildSelectedPkgs(importers, params, opts) // eslint-disable-line
  )
  if (opts.lockfileDir) {
    const importers = await getImporters()
    await rebuild(
      importers,
      {
        ...rebuildOpts,
        pending: opts.pending === true,
      }
    )
    return
  }
  let firstError: Error | undefined
  await scheduleGraph(projectDependencies, {
    bail: opts.bail !== false,
    concurrency: getWorkspaceConcurrency(opts.workspaceConcurrency),
    continueOnFailure: opts.bail === false,
    runNode: async (rootDir): Promise<TaskCompletion> => {
      try {
        if (opts.ignoredPackages?.has(rootDir)) return 'passed'
        result[rootDir] = { status: 'running' }
        const { manifest } = opts.selectedProjectsGraph[rootDir].package
        const localConfig = manifest.name ? projectConfigRecord[manifest.name] : undefined
        await rebuild(
          [{ buildIndex: 0, manifest: manifestsByPath[rootDir].manifest, rootDir }],
          {
            ...rebuildOpts,
            ...localConfig,
            dir: rootDir,
            pending: opts.pending === true,
          }
        )
        result[rootDir].status = 'passed'
        return 'passed'
      } catch (err: unknown) {
        assert(util.types.isNativeError(err))
        const errWithPrefix = Object.assign(err, { prefix: rootDir })
        logger.info(errWithPrefix)
        result[rootDir] = {
          status: 'failure',
          error: errWithPrefix,
          message: err.message,
          prefix: rootDir,
        }
        firstError ??= errWithPrefix
        return opts.bail === false ? 'failed' : 'aborted'
      }
    },
    onNodeSkipped: () => {},
  })
  if (opts.bail !== false && firstError != null) throw firstError

  throwOnFail(result)
}
