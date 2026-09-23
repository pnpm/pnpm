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

/** The project install stages run during install when devDependencies are excluded (e.g. `--prod`). */
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
}

export async function runLifecycleHooksConcurrently (
  params: {
    childConcurrency: number
    importers: Importer[]
    opts: RunLifecycleHooksConcurrentlyOptions
    projectDependencies?: Map<ProjectRootDir, ProjectRootDir[]>
    stages: string[]
    /**
     * Whether to skip linking bins into node_modules/.bin ahead of running scripts.
     * Used by pre-uninstall hooks so node_modules is not modified before unlinking.
     */
    skipBinLinking?: boolean
    /**
     * The project whose `preinstall` already ran before the install began,
     * so its run here starts at the stage after it.
     */
    projectWithPreinstallRan?: string
  }
): Promise<void> {
  const { childConcurrency, importers, opts, projectDependencies, projectWithPreinstallRan, skipBinLinking, stages } = params
  const importersByRootDir = new Map(importers.map((importer) => [importer.rootDir, importer]))
  const dependencies = projectDependencies == null
    ? dependenciesFromBuildIndexes(importers)
    : new Map(importers.map(({ rootDir }) => [
      rootDir,
      (projectDependencies.get(rootDir) ?? []).filter((dependency) => importersByRootDir.has(dependency)),
    ]))
  let firstError: unknown
  await scheduleGraph(dependencies, {
    bail: true,
    concurrency: childConcurrency,
    runNode: async (rootDir): Promise<TaskCompletion> => {
      const { manifest, modulesDir, stages: importerStages, targetDirs } = importersByRootDir.get(rootDir)!
      try {
        const binsDir = path.join(modulesDir, '.bin')
        if (!skipBinLinking) {
          // We are linking the bin files, in case they were created by lifecycle scripts of other workspace packages.
          await linkBins(modulesDir, binsDir, {
            extraNodePaths: opts.extraNodePaths,
            projectModulesDir: await getProjectNodePath({ modulesDir, rootDir }, opts),
            allowExoticManifests: true,
            preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
            projectManifest: manifest,
            warn: (message: string) => {
              logger.warn({ message, prefix: rootDir })
            },
          })
        }
        const runLifecycleHookOpts: RunLifecycleHookOptions = {
          ...opts,
          depPath: rootDir,
          extraEnv: { ...opts.extraEnv, ...await makeProjectNodePathOption({ modulesDir, rootDir }, opts) },
          pkgRoot: rootDir,
          rootModulesDir: modulesDir,
          wdBinDir: binsDir,
        }
        let isBuilt = false
        for (const stage of (importerStages ?? stages)) {
          if (stage === 'preinstall' && rootDir === projectWithPreinstallRan) {
            if (manifest.scripts?.preinstall != null) isBuilt = true
            continue
          }
          if (await runLifecycleHook(stage, manifest, runLifecycleHookOpts)) { // eslint-disable-line no-await-in-loop
            isBuilt = true
          }
        }
        if (targetDirs == null || targetDirs.length === 0 || !isBuilt) return 'passed'
        // Re-import only the freshly-built source — fetchFromDir already
        // excludes the source's node_modules/. `keepModulesDir: true` makes
        // importIndexedDir skip the destructive makeEmptyDir fast path
        // (#11088) and preserve the target's existing node_modules (bin
        // symlinks + transitive deps from the initial install) via its
        // staging/move path. Replaces the old scanDir-into-filesMap
        // workaround (#4299) that the fast path then wiped, causing ENOENT
        // on .bin/<tool>. Stays on storeController.importPackage so source
        // files keep their hardlinks (no copy-loop).
        const filesResponse = await fetchFromDir(rootDir, { resolveSymlinks: opts.resolveSymlinksInInjectedDirs })
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
        return 'passed'
      } catch (error: unknown) {
        firstError ??= error
        return 'aborted'
      }
    },
    onNodeSkipped: () => {},
  })
  if (firstError != null) throw firstError
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
