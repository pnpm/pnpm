import fs from 'fs'
import { linkBins } from '@pnpm/link-bins'
import { logger } from '@pnpm/logger'
import path from 'path'
import { fetchFromDir } from '@pnpm/directory-fetcher'
import { type StoreController } from '@pnpm/store-controller-types'
import { type ProjectManifest, type ProjectRootDir } from '@pnpm/types'
import runGroups from 'run-groups'
import { runLifecycleHook, type RunLifecycleHookOptions } from './runLifecycleHook'

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
        const runLifecycleHookOpts = {
          ...opts,
          depPath: rootDir,
          pkgRoot: rootDir,
          rootModulesDir: modulesDir,
        }
        let isBuilt = false
        for (const stage of (importerStages ?? stages)) {
          if (!manifest.scripts?.[stage]) continue
          await runLifecycleHook(stage, manifest, runLifecycleHookOpts) // eslint-disable-line no-await-in-loop
          isBuilt = true
        }
        if (targetDirs == null || targetDirs.length === 0 || !isBuilt) return
        const filesResponse = await fetchFromDir(rootDir, { resolveSymlinks: opts.resolveSymlinksInInjectedDirs })
        await Promise.all(
          targetDirs.map(async (targetDir) => {
            const targetModulesDir = path.join(targetDir, 'node_modules')
            const nodeModulesIndex = {}
            if (fs.existsSync(targetModulesDir)) {
              // If the target directory contains a node_modules directory
              // (it may happen when the hoisted node linker is used)
              // then we need to preserve this node_modules.
              // So we scan this node_modules directory and  pass it as part of the new package.
              await scanDir('node_modules', targetModulesDir, targetModulesDir, nodeModulesIndex)
            }
            return opts.storeController.importPackage(targetDir, {
              filesResponse: {
                resolvedFrom: 'local-dir',
                ...filesResponse,
                filesIndex: {
                  ...filesResponse.filesIndex,
                  ...nodeModulesIndex,
                },
              },
              force: false,
            })
          })
        )
      }
    )
  })
  await runGroups(childConcurrency, groups)
}

async function scanDir (prefix: string, rootDir: string, currentDir: string, index: Record<string, string>): Promise<void> {
  const files = await fs.promises.readdir(currentDir)
  await Promise.all(files.map(async (file) => {
    const fullPath = path.join(currentDir, file)
    const stat = await fs.promises.stat(fullPath)
    if (stat.isDirectory()) {
      return scanDir(prefix, rootDir, fullPath, index)
    }
    if (stat.isFile()) {
      const relativePath = path.relative(rootDir, fullPath)
      index[path.join(prefix, relativePath)] = fullPath
    }
  }))
}
