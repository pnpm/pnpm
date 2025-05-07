import path from 'path'
import { type Modules, readModulesManifest } from '@pnpm/modules-yaml'
import { type IgnoredBuildsCommandOpts } from './ignoredBuilds'

export interface GetAutomaticallyIgnoredBuildsResult {
  automaticallyIgnoredBuilds: string[] | null
  modulesDir: string
  modulesManifest: Modules | null
}

export async function getAutomaticallyIgnoredBuilds (opts: IgnoredBuildsCommandOpts): Promise<GetAutomaticallyIgnoredBuildsResult> {
  const modulesDir = getModulesDir(opts)
  const modulesManifest = await readModulesManifest(modulesDir)
  return {
    automaticallyIgnoredBuilds: modulesManifest && (modulesManifest.ignoredBuilds ?? []),
    modulesDir,
    modulesManifest,
  }
}

function getModulesDir (opts: IgnoredBuildsCommandOpts): string {
  return opts.modulesDir ?? path.join(opts.lockfileDir ?? opts.dir, 'node_modules')
}
