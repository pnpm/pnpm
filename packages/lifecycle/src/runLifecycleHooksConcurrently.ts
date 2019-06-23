import { ImporterManifest } from '@pnpm/types'
import runGroups from 'run-groups'
import runLifecycleHook from './runLifecycleHook'

export default async function runLifecycleHooksConcurrently (
  stages: string[],
  importers: Array<{ buildIndex: number, manifest: ImporterManifest, prefix: string, modulesDir: string }>,
  childConcurrency: number,
  opts: {
    extraBinPaths?: string[],
    rawNpmConfig: object,
    stdio?: string,
    unsafePerm: boolean,
  },
) {
  const importersByBuildIndex = new Map<number, Array<{ prefix: string, manifest: ImporterManifest, modulesDir: string }>>()
  for (const importer of importers) {
    if (!importersByBuildIndex.has(importer.buildIndex)) {
      importersByBuildIndex.set(importer.buildIndex, [importer])
    } else {
      importersByBuildIndex.get(importer.buildIndex)!.push(importer)
    }
  }
  const sortedBuildIndexes = Array.from(importersByBuildIndex.keys()).sort()
  const groups = sortedBuildIndexes.map((buildIndex) => {
    const importers = importersByBuildIndex.get(buildIndex) as Array<{ prefix: string, manifest: ImporterManifest, modulesDir: string }>
    return importers.map(({ manifest, modulesDir, prefix }) =>
      async () => {
        const runLifecycleHookOpts = {
          depPath: prefix,
          extraBinPaths: opts.extraBinPaths,
          pkgRoot: prefix,
          rawNpmConfig: opts.rawNpmConfig,
          rootNodeModulesDir: modulesDir,
          stdio: opts.stdio,
          unsafePerm: opts.unsafePerm,
        }
        for (const stage of stages) {
          if (!manifest.scripts || !manifest.scripts[stage]) continue
          await runLifecycleHook(stage, manifest, runLifecycleHookOpts)
        }
      }
    )
  })
  await runGroups(childConcurrency, groups)
}
