import { fetchFromDir } from '@pnpm/directory-fetcher'
import { StoreController } from '@pnpm/store-controller-types'
import { ProjectManifest } from '@pnpm/types'
import runGroups from 'run-groups'
import runLifecycleHook, { RunLifecycleHookOptions } from './runLifecycleHook'

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

export default async function runLifecycleHooksConcurrently (
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
          targetDirs.map((targetDir) => opts.storeController.importPackage(targetDir, {
            filesResponse: {
              fromStore: false,
              ...filesResponse,
            },
            force: false,
          }))
        )
      }
    )
  })
  await runGroups(childConcurrency, groups)
}
