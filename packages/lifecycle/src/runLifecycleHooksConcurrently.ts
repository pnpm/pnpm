import { PackageJson } from '@pnpm/types'
import runGroups from 'run-groups'
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
  const sortedBuildIndexes = Array.from(importersByBuildIndex.keys()).sort()
  const groups = sortedBuildIndexes.map((buildIndex) => {
    const importers = importersByBuildIndex.get(buildIndex) as Array<{ prefix: string, pkg: PackageJson, modulesDir: string }>
    return importers.map((importer) =>
      async () => {
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
      }
    )
  })
  await runGroups(childConcurrency, groups)
}
