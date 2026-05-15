import fs from 'node:fs'
import path from 'node:path'

import { linkBins } from '@pnpm/bins.linker'
import { fetchFromDir } from '@pnpm/fetching.directory-fetcher'
import { logger } from '@pnpm/logger'
import type { FilesMap } from '@pnpm/store.cafs-types'
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
        // After the workspace package's prepare/postinstall script runs in
        // its source dir, mirror the post-script source tree into each of
        // its injected target dirs so consumers see the freshly-built dist/.
        //
        // We do this with a plain directory-mirror (file-by-file overlay)
        // instead of routing through storeController.importPackage:
        //
        //   - The old `scanDir → filesMap` + fast-path-import flow (#4299)
        //     broke when #11088 made the makeEmptyDir fast path default,
        //     because the scanned source paths lived inside the very dir
        //     about to be wiped — ENOENT on `.bin/<tool>` symlinks.
        //   - Routing through `keepModulesDir: true` (staging path) avoids
        //     the wipe but its `moveOrMergeModulesDirs` step trips over
        //     `.bin/<tool>` symlinks under cross-device EXDEV fallbacks on
        //     virtualized filesystems (e.g. CI pod overlays).
        //
        // A direct overlay is both simpler and more robust: we never touch
        // the target's existing `node_modules` (the bin links + transitive
        // deps set up by the initial install stay intact), and we never go
        // through a tmp-dir swap that has to dance around symlinks.
        const filesResponse = await fetchFromDir(rootDir, { resolveSymlinks: opts.resolveSymlinksInInjectedDirs })
        await Promise.all(
          targetDirs.map(async (targetDir) => mirrorFilesIntoTarget(filesResponse.filesMap, targetDir))
        )
      }
    )
  })
  await runGroups(childConcurrency, groups)
}

// Mirror each file in filesMap from its source path into targetDir, creating
// parent directories as needed. Source paths come from `fetchFromDir`, which
// already excludes the source's `node_modules/` subtree — so this never
// touches the target's existing `node_modules/` (its bin links and installed
// deps stay intact).
//
// Implementation notes:
//   - We use `fs.copyFileSync` with the source path as-is. `fetchFromDir`
//     records symlink paths verbatim (resolveSymlinks defaults to false),
//     so a broken symlink in the source would surface as ENOENT here just
//     like it would in a regular `pnpm install`. That's the correct
//     behavior — the bug we're fixing was distinct (re-importing into a
//     tmp dir then renaming, which tripped over `.bin/` symlinks).
//   - We `unlinkSync` existing destinations first to guarantee an
//     overwrite. Without this, `copyFileSync` would error on existing
//     symlinks (since the kernel-level copy can't replace a symlink atomically).
function mirrorFilesIntoTarget (filesMap: FilesMap, targetDir: string): void {
  const dirsToCreate = new Set<string>()
  for (const relPath of filesMap.keys()) {
    const dir = path.dirname(relPath)
    if (dir !== '.') dirsToCreate.add(dir)
  }
  for (const dir of Array.from(dirsToCreate).sort((a, b) => a.length - b.length)) {
    fs.mkdirSync(path.join(targetDir, dir), { recursive: true })
  }
  for (const [relPath, srcAbs] of filesMap) {
    const destAbs = path.join(targetDir, relPath)
    try {
      fs.unlinkSync(destAbs)
    } catch (err: unknown) {
      // Missing dest is fine — we're about to create it. Anything else
      // (EISDIR, EACCES, etc.) we want to surface.
      if (!(err instanceof Error) || !('code' in err) || err.code !== 'ENOENT') throw err
    }
    fs.copyFileSync(srcAbs, destAbs)
  }
}
