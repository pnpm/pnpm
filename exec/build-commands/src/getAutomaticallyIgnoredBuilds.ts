import path from 'path'
import { readModulesManifest } from '@pnpm/modules-yaml'
import { type IgnoredBuildsCommandOpts } from './ignoredBuilds'

export async function getAutomaticallyIgnoredBuilds (opts: IgnoredBuildsCommandOpts): Promise<null | string[]> {
  const modulesManifest = await readModulesManifest(opts.modulesDir ?? path.join(opts.dir, 'node_modules'))
  if (modulesManifest == null) {
    return null
  }
  return modulesManifest?.ignoredBuilds ?? []
}
