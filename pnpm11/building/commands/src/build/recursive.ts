import assert from 'node:assert'

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
import { isError } from '@pnpm/error'
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
| 'patchedDependencies'
| 'packageConfigs'
| 'registriesByScope'
| 'sharedWorkspaceLockfile'
| 'virtualStoreDir'
> & Pick<ConfigContext,
| 'hooks'
| 'rootProjectManifest'
| 'rootProjectManifestDir'
> & {
  pending?: boolean
} & Partial<Pick<Config, 'bail' | 'sort' | 'workspaceConcurrency'>>

type RecursiveRebuildFullOpts = RecursiveRebuildOpts & {
  ignoredPackages?: Set<string>
} & Required<Pick<ConfigContext, 'selectedProjectsGraph'>> & Pick<ConfigContext, 'allProjectsGraph' | 'prodAllProjectsGraph' | 'prodOnlySelectedProjectDirs'> & Required<Pick<Config, 'workspaceDir'>>

type ManifestsByPath = Record<string, Omit<Project, 'rootDir' | 'rootDirRealPath'>>

type ProjectDependencies = Map<ProjectRootDir, ProjectRootDir[]>

type Rebuild = (importers: Parameters<typeof rebuildAll>[0], opts: BuildOptions) => Promise<unknown>

export async function recursiveRebuild (
  allProjects: Project[],
  params: string[],
  opts: RecursiveRebuildFullOpts
): Promise<void> {
  if (allProjects.length === 0) {
    // It might make sense to throw an exception in this case
    return
  }

  const pkgs = Object.values(opts.selectedProjectsGraph).map((wsPkg) => wsPkg.package)

  if (pkgs.length === 0) {
    return
  }
  const manifestsByPath = indexManifestsByRootDir(pkgs)

  const projectDependencies = getProjectDependencies(opts)

  const store = await createStoreController(opts)

  const rebuildOpts = Object.assign(opts, {
    ownLifecycleHooksStdio: 'pipe',
    pruneLockfileImporters: ((opts.ignoredPackages == null) || opts.ignoredPackages.size === 0) &&
      pkgs.length === allProjects.length,
    storeController: store.ctrl,
    storeDir: store.dir,
    projectDependencies,
  }) as BuildOptions

  const projectConfigRecord = createProjectConfigRecord(opts) ?? {}

  const rebuild: Rebuild = (
    params.length === 0
      ? rebuildAll
    : (importers: any, opts: any) => buildSelectedPkgs(importers, params, opts) // eslint-disable-line
  )
  if (opts.lockfileDir) {
    await rebuild(
      getImporters({ projectDependencies, manifestsByPath, ignoredPackages: opts.ignoredPackages }),
      {
        ...rebuildOpts,
        pending: opts.pending === true,
      }
    )
    return
  }
  await rebuildProjectsInOrder({ opts, projectDependencies, manifestsByPath, rebuildOpts, projectConfigRecord, rebuild })
}

function indexManifestsByRootDir (pkgs: Project[]): ManifestsByPath {
  const manifestsByPath: ManifestsByPath = {}
  for (const { rootDir, manifest, writeProjectManifest } of pkgs) {
    manifestsByPath[rootDir] = { manifest, writeProjectManifest }
  }
  return manifestsByPath
}

function getProjectDependencies (opts: RecursiveRebuildFullOpts): ProjectDependencies {
  return opts.sort !== false
    ? filteredProjectsDependencies(opts)
    : new Map((Object.keys(opts.selectedProjectsGraph).sort() as ProjectRootDir[]).map((rootDir) => [rootDir, []]))
}

interface GetImportersOptions {
  projectDependencies: ProjectDependencies
  manifestsByPath: ManifestsByPath
  ignoredPackages?: Set<string>
}

function getImporters ({ projectDependencies, manifestsByPath, ignoredPackages }: GetImportersOptions) {
  return [...projectDependencies.keys()]
    .filter((rootDir) => !ignoredPackages?.has(rootDir))
    .map((rootDir) => ({
      buildIndex: 0,
      manifest: manifestsByPath[rootDir].manifest,
      rootDir,
    }))
}

interface RebuildProjectsInOrderOptions {
  opts: RecursiveRebuildFullOpts
  projectDependencies: ProjectDependencies
  manifestsByPath: ManifestsByPath
  rebuildOpts: BuildOptions
  projectConfigRecord: NonNullable<ReturnType<typeof createProjectConfigRecord>>
  rebuild: Rebuild
}

async function rebuildProjectsInOrder (ctx: RebuildProjectsInOrderOptions): Promise<void> {
  const { opts, projectDependencies } = ctx
  const throwOnFail = throwOnCommandFail.bind(null, 'pnpm recursive rebuild')
  const result: RecursiveSummary = {}
  let firstError: Error | undefined
  await scheduleGraph(projectDependencies, {
    bail: opts.bail !== false,
    concurrency: getWorkspaceConcurrency(opts.workspaceConcurrency),
    continueOnFailure: opts.bail === false,
    runNode: async (rootDir): Promise<TaskCompletion> => {
      try {
        if (opts.ignoredPackages?.has(rootDir)) return 'passed'
        result[rootDir] = { status: 'running' }
        await rebuildProject(ctx, rootDir)
        result[rootDir].status = 'passed'
        return 'passed'
      } catch (err: unknown) {
        assert(isError(err))
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

async function rebuildProject (ctx: RebuildProjectsInOrderOptions, rootDir: ProjectRootDir): Promise<void> {
  const { manifest } = ctx.opts.selectedProjectsGraph[rootDir].package
  const localConfig = manifest.name ? ctx.projectConfigRecord[manifest.name] : undefined
  await ctx.rebuild(
    [{ buildIndex: 0, manifest: ctx.manifestsByPath[rootDir].manifest, rootDir }],
    {
      ...ctx.rebuildOpts,
      ...localConfig,
      dir: rootDir,
      pending: ctx.opts.pending === true,
    }
  )
}
