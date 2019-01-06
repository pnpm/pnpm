import { PackageJson } from '@pnpm/types'
import pLimit = require('p-limit')
import runLifecycleHook from './runLifecycleHook'

export default async function runLifecycleHooksConcurrently (
  stages: string[],
  importers: Array<{ buildIndex: number, pkg: PackageJson, prefix: string, modulesDir: string }>,
  childConcurrency: number,
  opts: {
    rawNpmConfig: object,
    stdio?: string,
    unsafePerm: boolean,
  },
) {
  const importersByBuildIndex = new Map<number, Array<{ prefix: string, pkg: PackageJson, modulesDir: string }>>()
  for (const importer of importers) {
    if (!importersByBuildIndex.has(importer.buildIndex)) {
      importersByBuildIndex.set(importer.buildIndex, [importer])
    } else {
      importersByBuildIndex.get(importer.buildIndex)!.push(importer)
    }
  }
  const limitChild = pLimit(childConcurrency)
  const sortedBuildIndexes = Array.from(importersByBuildIndex.keys()).sort()
  for (const buildIndex of sortedBuildIndexes) {
    const importers = importersByBuildIndex.get(buildIndex) as Array<{ prefix: string, pkg: PackageJson, modulesDir: string }>
    await Promise.all(
      importers.map((importer) => limitChild(async () => {
        const runLifecycleHookOpts = {
          depPath: importer.prefix,
          pkgRoot: importer.prefix,
          rawNpmConfig: opts.rawNpmConfig,
          rootNodeModulesDir: importer.modulesDir,
          stdio: opts.stdio,
          unsafePerm: opts.unsafePerm,
        }
        for (const stage of stages) {
          if (!importer.pkg.scripts || !importer.pkg.scripts[stage]) continue
          await runLifecycleHook(stage, importer.pkg, runLifecycleHookOpts)
        }
      }))
    )
  }
}
