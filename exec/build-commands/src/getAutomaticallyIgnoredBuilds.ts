import path from 'path'
import { parse } from '@pnpm/dependency-path'
import { type Modules, readModulesManifest } from '@pnpm/modules-yaml'
import { type IgnoredBuildsCommandOpts } from './ignoredBuilds.js'

export interface GetAutomaticallyIgnoredBuildsResult {
  automaticallyIgnoredBuilds: string[] | null
  modulesDir: string
  modulesManifest: Modules | null
}

export async function getAutomaticallyIgnoredBuilds (opts: IgnoredBuildsCommandOpts): Promise<GetAutomaticallyIgnoredBuildsResult> {
  const modulesDir = getModulesDir(opts)
  const modulesManifest = await readModulesManifest(modulesDir)
  const ignoredPkgNames = new Set<string>()
  if (modulesManifest?.ignoredBuilds) {
    for (const depPath of modulesManifest?.ignoredBuilds) {
      ignoredPkgNames.add(parse(depPath).name ?? depPath)
    }
  }
  return {
    automaticallyIgnoredBuilds: Array.from(ignoredPkgNames),
    modulesDir,
    modulesManifest,
  }
}

function getModulesDir (opts: IgnoredBuildsCommandOpts): string {
  return opts.modulesDir ?? path.join(opts.lockfileDir ?? opts.dir, 'node_modules')
}
