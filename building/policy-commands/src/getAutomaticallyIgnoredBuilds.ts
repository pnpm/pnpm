import path from 'path'
import { parse } from '@pnpm/dependency-path'
import { type Modules, readModulesManifest } from '@pnpm/modules-yaml'
import type { IgnoredBuildsCommandOpts } from './ignoredBuilds.js'

export interface GetAutomaticallyIgnoredBuildsResult {
  automaticallyIgnoredBuilds: string[] | null
  modulesDir: string
  modulesManifest: Modules | null
}

export async function getAutomaticallyIgnoredBuilds (opts: IgnoredBuildsCommandOpts): Promise<GetAutomaticallyIgnoredBuildsResult> {
  const modulesDir = getModulesDir(opts)
  const modulesManifest = await readModulesManifest(modulesDir)
  let automaticallyIgnoredBuilds: null | string[]
  if (modulesManifest?.ignoredBuilds) {
    const ignoredPkgNames = new Set<string>()
    for (const depPath of modulesManifest.ignoredBuilds) {
      ignoredPkgNames.add(parse(depPath).name ?? depPath)
    }
    automaticallyIgnoredBuilds = Array.from(ignoredPkgNames)
  } else {
    automaticallyIgnoredBuilds = null
  }
  return {
    automaticallyIgnoredBuilds,
    modulesDir,
    modulesManifest,
  }
}

function getModulesDir (opts: IgnoredBuildsCommandOpts): string {
  return opts.modulesDir ?? path.join(opts.lockfileDir ?? opts.dir, 'node_modules')
}
