import path from 'node:path'

import { getProjectNodePath, linkBins } from '@pnpm/bins.linker'
import { fetchFromDir } from '@pnpm/fetching.directory-fetcher'
import { logger } from '@pnpm/logger'
import type { StoreController } from '@pnpm/store.controller-types'
import type { ProjectManifest, ProjectRootDir } from '@pnpm/types'
import { scheduleGraph, type TaskCompletion } from '@pnpm/workspace.task-scheduler'

import { makeProjectNodePathOption } from './makeProjectNodePathOption.js'
import { runLifecycleHook, type RunLifecycleHookOptions } from './runLifecycleHook.js'

export type RunLifecycleHooksConcurrentlyOptions = Omit<RunLifecycleHookOptions,
| 'depPath'
| 'pkgRoot'
| 'rootModulesDir'
> & {
  resolveSymlinksInInjectedDirs?: boolean
  storeController: StoreController
  extendNodePath?: boolean
  extraNodePaths?: string[]
  preferSymlinkedExecutables?: boolean
}

/** The project stages `pnpm remove` runs before it unlinks anything. */
export const PRE_UNINSTALL_STAGES = ['preuninstall', 'uninstall']

/** The project stage `pnpm remove` runs after unlinking. */
export const POST_UNINSTALL_STAGES = ['postuninstall']

/** The project install stages run during deploy or when devDependencies are excluded (e.g. `--prod`). */
export const PROJECT_INSTALL_STAGES = ['preinstall', 'install', 'postinstall']

/** The project lifecycle stages run during full install. */
export const PROJECT_LIFECYCLE_STAGES = ['preinstall', 'install', 'postinstall', 'preprepare', 'prepare', 'postprepare']

export interface Importer {
  buildIndex: number
  manifest: ProjectManifest
  rootDir: ProjectRootDir
  modulesDir: string
  stages?: string[]
  targetDirs?: string[]
  publishTargetDirs?: string[]
}

export async function runLifecycleHooksConcurrently (
  params: {
    childConcurrency: number
    importers: Importer[]
    opts: RunLifecycleHooksConcurrentlyOptions
    projectDependencies?: Map<ProjectRootDir, ProjectRootDir[]>
    stages: string[]
    skipBinLinking?: boolean
    projectWithPreinstallRan?: string
  }
): Promise<void> {
  const importersByRootDir = new Map(params.importers.map((importer) => [importer.rootDir, importer]))
  const dependencies = buildDependenciesMap(params.importers, importersByRootDir, params.projectDependencies)
  let firstError: unknown
  await scheduleGraph(dependencies, {
    bail: true,
    concurrency: params.childConcurrency,
    runNode: async (rootDir): Promise<TaskCompletion> => {
      try {
        return await runImporterNode({
          importer: importersByRootDir.get(rootDir)!,
          opts: params.opts,
          projectWithPreinstallRan: params.projectWithPreinstallRan,
          rootDir,
          skipBinLinking: params.skipBinLinking,
          stages: params.stages,
        })
      } catch (error: unknown) {
        firstError ??= error
        return 'aborted'
      }
    },
    onNodeSkipped: () => {},
  })
  if (firstError != null) throw firstError
}

function buildDependenciesMap (
  importers: Importer[],
  importersByRootDir: Map<ProjectRootDir, Importer>,
  projectDependencies?: Map<ProjectRootDir, ProjectRootDir[]>
): Map<ProjectRootDir, ProjectRootDir[]> {
  if (projectDependencies == null) {
    return dependenciesFromBuildIndexes(importers)
  }
  return new Map(importers.map(({ rootDir }) => [
    rootDir,
    (projectDependencies.get(rootDir) ?? []).filter((dep) => importersByRootDir.has(dep)),
  ]))
}

interface RunImporterNodeParams {
  importer: Importer
  opts: RunLifecycleHooksConcurrentlyOptions
  projectWithPreinstallRan?: string
  rootDir: ProjectRootDir
  skipBinLinking?: boolean
  stages: string[]
}

async function runImporterNode (params: RunImporterNodeParams): Promise<TaskCompletion> {
  const { importer, opts, rootDir } = params
  const binsDir = await prepareImporterBins(rootDir, importer, opts, params.skipBinLinking)
  const runOpts = await buildRunLifecycleHookOpts(rootDir, importer, binsDir, opts)

  const isBuilt = await executeImporterStages({
    importerStages: importer.stages ?? params.stages,
    manifest: importer.manifest,
    projectWithPreinstallRan: params.projectWithPreinstallRan,
    rootDir,
    runOpts,
  })

  const publishDir = importer.manifest.publishConfig?.directory != null && importer.manifest.publishConfig?.linkDirectory !== false
    ? path.resolve(rootDir, importer.manifest.publishConfig.directory)
    : undefined

  const targetGroups = [
    { sourceDir: rootDir, targetDirs: importer.targetDirs ?? [] },
    { sourceDir: publishDir, targetDirs: importer.publishTargetDirs ?? [] },
  ].filter((group): group is { sourceDir: string, targetDirs: string[] } => group.sourceDir != null && group.targetDirs.length > 0)

  if (targetGroups.length === 0 || !isBuilt) return 'passed'
  await reimportTargetGroups(targetGroups, opts)
  return 'passed'
}

async function prepareImporterBins (
  rootDir: ProjectRootDir,
  importer: Importer,
  opts: RunLifecycleHooksConcurrentlyOptions,
  skipBinLinking?: boolean
): Promise<string> {
  const binsDir = path.join(importer.modulesDir, '.bin')
  if (!skipBinLinking) {
    await linkBins(importer.modulesDir, binsDir, {
      extraNodePaths: opts.extraNodePaths,
      projectModulesDir: await getProjectNodePath({ modulesDir: importer.modulesDir, rootDir }, opts),
      allowExoticManifests: true,
      preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
      projectManifest: importer.manifest,
      warn: (message: string) => {
        logger.warn({ message, prefix: rootDir })
      },
    })
  }
  return binsDir
}

async function buildRunLifecycleHookOpts (
  rootDir: ProjectRootDir,
  importer: Importer,
  binsDir: string,
  opts: RunLifecycleHooksConcurrentlyOptions
): Promise<RunLifecycleHookOptions> {
  return {
    ...opts,
    depPath: rootDir,
    extraEnv: { ...opts.extraEnv, ...await makeProjectNodePathOption({ modulesDir: importer.modulesDir, rootDir }, opts) },
    pkgRoot: rootDir,
    rootModulesDir: importer.modulesDir,
    wdBinDir: binsDir,
  }
}

interface ExecuteImporterStagesParams {
  importerStages: string[]
  manifest: ProjectManifest
  projectWithPreinstallRan?: string
  rootDir: ProjectRootDir
  runOpts: RunLifecycleHookOptions
}

async function executeImporterStages (params: ExecuteImporterStagesParams): Promise<boolean> {
  const { importerStages, manifest, projectWithPreinstallRan, rootDir, runOpts } = params
  let isBuilt = false
  for (const stage of importerStages) {
    if (stage === 'preinstall' && rootDir === projectWithPreinstallRan) {
      if (manifest.scripts?.preinstall != null) isBuilt = true
      continue
    }
    // eslint-disable-next-line no-await-in-loop -- lifecycle stages of one project run in order
    if (await runLifecycleHook(stage, manifest, runOpts)) {
      isBuilt = true
    }
  }
  return isBuilt
}

interface TargetGroup {
  sourceDir: string
  targetDirs: string[]
}

async function reimportTargetGroups (targetGroups: TargetGroup[], opts: RunLifecycleHooksConcurrentlyOptions): Promise<void> {
  await Promise.all(
    targetGroups.map(async ({ sourceDir, targetDirs }) => {
      const filesResponse = await fetchFromDir(sourceDir, { resolveSymlinks: opts.resolveSymlinksInInjectedDirs })
      await Promise.all(
        targetDirs.map(async (targetDir) =>
          opts.storeController.importPackage(targetDir, {
            filesResponse: {
              resolvedFrom: 'local-dir',
              ...filesResponse,
            },
            force: false,
            keepModulesDir: true,
          })
        )
      )
    })
  )
}

function dependenciesFromBuildIndexes (importers: Importer[]): Map<ProjectRootDir, ProjectRootDir[]> {
  const groups = new Map<number, ProjectRootDir[]>()
  for (const { buildIndex, rootDir } of importers) {
    const group = groups.get(buildIndex) ?? []
    group.push(rootDir)
    groups.set(buildIndex, group)
  }
  const dependencies = new Map<ProjectRootDir, ProjectRootDir[]>()
  let previous: ProjectRootDir[] = []
  for (const buildIndex of [...groups.keys()].sort((left, right) => left - right)) {
    const group = groups.get(buildIndex)!
    for (const rootDir of group) dependencies.set(rootDir, previous)
    previous = group
  }
  return dependencies
}
