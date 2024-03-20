import fs from 'node:fs'
import path from 'node:path'

import runGroups from 'run-groups'

import { logger } from '@pnpm/logger'
import { linkBins } from '@pnpm/link-bins'
import { fetchFromDir } from '@pnpm/directory-fetcher'
import { LifecycleImporter, RunLifecycleHooksConcurrentlyOptions } from '@pnpm/types'

import {
  runLifecycleHook,
} from './runLifecycleHook'

export async function runLifecycleHooksConcurrently(
  stages: string[],
  importers: LifecycleImporter[],
  childConcurrency: number,
  opts: RunLifecycleHooksConcurrentlyOptions
) {
  const importersByBuildIndex = new Map<number, LifecycleImporter[]>()

  for (const importer of importers) {
    if (typeof importer.buildIndex !== 'undefined') {
      if (importersByBuildIndex.has(importer.buildIndex)) {
        importersByBuildIndex.get(importer.buildIndex)?.push(importer)
      } else {
        importersByBuildIndex.set(importer.buildIndex, [importer])
      }
    }
  }

  const sortedBuildIndexes = Array.from(importersByBuildIndex.keys()).sort(
    (a, b) => a - b
  )

  const groups = sortedBuildIndexes.map((buildIndex: number): (() => Promise<void>)[] | undefined => {
    const importers = importersByBuildIndex.get(buildIndex)

    return importers?.map(
      ({ manifest, modulesDir, rootDir, stages: importerStages, targetDirs }: LifecycleImporter): () => Promise<void> => {
        return async (): Promise<void> => {
          // We are linking the bin files, in case they were created by lifecycle scripts of other workspace packages.
          await linkBins(modulesDir, path.join(modulesDir ?? '', '.bin'), {
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

          for (const stage of importerStages ?? stages) {
            if (!manifest?.scripts?.[stage]) {
              continue
            }

            await runLifecycleHook(stage, manifest, runLifecycleHookOpts) // eslint-disable-line no-await-in-loop

            isBuilt = true
          }

          if (targetDirs == null || targetDirs.length === 0 || !isBuilt) {
            return
          }

          const filesResponse = await fetchFromDir(rootDir, {
            resolveSymlinks: opts.resolveSymlinksInInjectedDirs,
          })

          await Promise.all(
            targetDirs.map(async (targetDir: string): Promise<{
              isBuilt: boolean;
              importMethod?: string | undefined;
            }> => {
              const targetModulesDir = path.join(targetDir, 'node_modules')

              const nodeModulesIndex = {}

              if (fs.existsSync(targetModulesDir)) {
                // If the target directory contains a node_modules directory
                // (it may happen when the hoisted node linker is used)
                // then we need to preserve this node_modules.
                // So we scan this node_modules directory and  pass it as part of the new package.
                await scanDir(
                  'node_modules',
                  targetModulesDir,
                  targetModulesDir,
                  nodeModulesIndex
                )
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
        };
      }
    )
  }).filter(Boolean)

  await runGroups(childConcurrency, groups)
}

async function scanDir(
  prefix: string,
  rootDir: string,
  currentDir: string,
  index: Record<string, string>
) {
  const files = await fs.promises.readdir(currentDir)

  await Promise.all(
    files.map(async (file: string): Promise<void> => {
      const fullPath = path.join(currentDir, file)

      const stat = await fs.promises.stat(fullPath)

      if (stat.isDirectory()) {
        return scanDir(prefix, rootDir, fullPath, index)
      }

      if (stat.isFile()) {
        const relativePath = path.relative(rootDir, fullPath)

        index[path.join(prefix, relativePath)] = fullPath
      }
    })
  )
}
