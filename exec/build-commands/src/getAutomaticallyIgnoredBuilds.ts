import path from 'path'
import { type Modules, readModulesManifest } from '@pnpm/modules-yaml'
import { type IgnoredBuildsCommandOpts } from './ignoredBuilds'

export function getModulesDir (opts: IgnoredBuildsCommandOpts): string {
  return opts.modulesDir ?? path.join(opts.lockfileDir ?? opts.dir, 'node_modules')
}

export function getAutomaticallyIgnoredBuildsFromModules (modulesManifest: Modules | null): string[] | null {
  return modulesManifest && (modulesManifest.ignoredBuilds ?? [])
}

export async function getAutomaticallyIgnoredBuilds (opts: IgnoredBuildsCommandOpts): Promise<string[] | null> {
  return getAutomaticallyIgnoredBuildsFromModules(await readModulesManifest(getModulesDir(opts)))
}
