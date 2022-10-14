import fs from 'fs'
import path from 'path'
import { fetchFromDir } from '@pnpm/directory-fetcher'
import { StoreController } from '@pnpm/store-controller-types'
import { ProjectManifest } from '@pnpm/types'
import runGroups from 'run-groups'
import { runLifecycleHook, RunLifecycleHookOptions } from './runLifecycleHook'

export type RunLifecycleHooksConcurrentlyOptions = Omit<RunLifecycleHookOptions,
| 'depPath'
| 'pkgRoot'
| 'rootModulesDir'
> & {
  storeController: StoreController
}

export interface Importer {
  buildIndex: number
  manifest: ProjectManifest
  rootDir: string
  modulesDir: string
  stages?: string[]
  targetDirs?: string[]
}

export async function runLifecycleHooksConcurrently (
  stages: string[],
  importers: Importer[],
  childConcurrency: number,
  opts: RunLifecycleHooksConcurrentlyOptions
) {
  const importersByBuildIndex = new Map<number, Importer[]>()
  for (const importer of importers) {
    if (!importersByBuildIndex.has(importer.buildIndex)) {
      importersByBuildIndex.set(importer.buildIndex, [importer])
    } else {
      importersByBuildIndex.get(importer.buildIndex)!.push(importer)
    }
  }
  const sortedBuildIndexes = Array.from(importersByBuildIndex.keys()).sort()
  const groups = sortedBuildIndexes.map((buildIndex) => {
    const importers = importersByBuildIndex.get(buildIndex)!
    return importers.map(({ manifest, modulesDir, rootDir, stages: importerStages, targetDirs }) =>
      async () => {
        const runLifecycleHookOpts = {
          ...opts,
          depPath: rootDir,
          pkgRoot: rootDir,
          rootModulesDir: modulesDir,
        }
        for (const stage of (importerStages ?? stages)) {
          if ((manifest.scripts == null) || !manifest.scripts[stage]) continue
          await runLifecycleHook(stage, manifest, runLifecycleHookOpts)
        }
        if (targetDirs == null || targetDirs.length === 0) return
        const filesResponse = await fetchFromDir(rootDir, {})
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
                fromStore: false,
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

async function scanDir (prefix: string, rootDir: string, currentDir: string, index: Record<string, string>) {
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
