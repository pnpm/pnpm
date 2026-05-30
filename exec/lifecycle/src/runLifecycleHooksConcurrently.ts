import path from 'node:path'

import { linkBins } from '@pnpm/bins.linker'
import { fetchFromDir } from '@pnpm/fetching.directory-fetcher'
import { logger } from '@pnpm/logger'
import type { StoreController } from '@pnpm/store.controller-types'
import type { ProjectManifest, ProjectRootDir } from '@pnpm/types'
import { runGroups } from 'run-groups'

import { runLifecycleHook, type RunLifecycleHookOptions } from './runLifecycleHook.js'

export type RunLifecycleHooksConcurrentlyOptions = Omit<RunLifecycleHookOptions,
| 'depPath'
| 'pkgRoot'
| 'rootModulesDir'
> & {
  resolveSymlinksInInjectedDirs?: boolean
  storeController: StoreController
  extraNodePaths?: string[]
  preferSymlinkedExecutables?: boolean
}

export interface Importer {
  buildIndex: number
  manifest: ProjectManifest
  rootDir: ProjectRootDir
  modulesDir: string
  stages?: string[]
  targetDirs?: string[]
}

export async function runLifecycleHooksConcurrently (
  stages: string[],
  importers: Importer[],
  childConcurrency: number,
  opts: RunLifecycleHooksConcurrentlyOptions
): Promise<void> {
  const importersByBuildIndex = new Map<number, Importer[]>()
  for (const importer of importers) {
    if (!importersByBuildIndex.has(importer.buildIndex)) {
      importersByBuildIndex.set(importer.buildIndex, [importer])
    } else {
      importersByBuildIndex.get(importer.buildIndex)!.push(importer)
    }
  }
  const sortedBuildIndexes = Array.from(importersByBuildIndex.keys()).sort((a, b) => a - b)
  const groups = sortedBuildIndexes.map((buildIndex) => {
    const importers = importersByBuildIndex.get(buildIndex)!
    return importers.map(({ manifest, modulesDir, rootDir, stages: importerStages, targetDirs }) =>
      async () => {
        // We are linking the bin files, in case they were created by lifecycle scripts of other workspace packages.
        await linkBins(modulesDir, path.join(modulesDir, '.bin'), {
          extraNodePaths: opts.extraNodePaths,
          allowExoticManifests: true,
          preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
          projectManifest: manifest,
          warn: (message: string) => {
            logger.warn({ message, prefix: rootDir })
          },
        })
        const runLifecycleHookOpts: RunLifecycleHookOptions = {
          ...opts,
          depPath: rootDir,
          pkgRoot: rootDir,
          rootModulesDir: modulesDir,
        }
        let isBuilt = false
        for (const stage of (importerStages ?? stages)) {
          if (await runLifecycleHook(stage, manifest, runLifecycleHookOpts)) { // eslint-disable-line no-await-in-loop
            isBuilt = true
          }
        }
        if (targetDirs == null || targetDirs.length === 0 || !isBuilt) return
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
      }
    )
  })
  await runGroups(childConcurrency, groups)
}
