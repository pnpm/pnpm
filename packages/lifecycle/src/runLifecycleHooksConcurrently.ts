import { ProjectManifest } from '@pnpm/types'
import runGroups from 'run-groups'
import runLifecycleHook, { RunLifecycleHookOptions } from './runLifecycleHook'

export default async function runLifecycleHooksConcurrently (
  stages: string[],
  importers: Array<{ buildIndex: number, manifest: ProjectManifest, rootDir: string, modulesDir: string }>,
  childConcurrency: number,
  opts: Pick<RunLifecycleHookOptions,
  | 'extraBinPaths'
  | 'rawConfig'
  | 'shellEmulator'
  | 'stdio'
  | 'unsafePerm'
  >
) {
  const importersByBuildIndex = new Map<number, Array<{ rootDir: string, manifest: ProjectManifest, modulesDir: string }>>()
  for (const importer of importers) {
    if (!importersByBuildIndex.has(importer.buildIndex)) {
      importersByBuildIndex.set(importer.buildIndex, [importer])
    } else {
      importersByBuildIndex.get(importer.buildIndex)!.push(importer)
    }
  }
  const sortedBuildIndexes = Array.from(importersByBuildIndex.keys()).sort()
  const groups = sortedBuildIndexes.map((buildIndex) => {
    const importers = importersByBuildIndex.get(buildIndex) as Array<{ rootDir: string, manifest: ProjectManifest, modulesDir: string }>
    return importers.map(({ manifest, modulesDir, rootDir }) =>
      async () => {
        const runLifecycleHookOpts = {
          depPath: rootDir,
          extraBinPaths: opts.extraBinPaths,
          pkgRoot: rootDir,
          rawConfig: opts.rawConfig,
          rootModulesDir: modulesDir,
          shellEmulator: opts.shellEmulator,
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
