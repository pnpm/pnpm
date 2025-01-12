import path from 'path'
import { type Config } from '@pnpm/config'
import { readModulesManifest } from '@pnpm/modules-yaml'
import renderHelp from 'render-help'

export type IgnoredBuildsCommandOpts = Pick<Config, 'modulesDir' | 'dir' | 'rootProjectManifest'>

export const commandNames = ['ignored-builds']

export function help (): string {
  return renderHelp({
    description: 'Print the list of packages with blocked build scripts',
    usages: [],
  })
}

export function cliOptionsTypes (): Record<string, unknown> {
  return {}
}

export function rcOptionsTypes (): Record<string, unknown> {
  return {}
}

export async function handler (opts: IgnoredBuildsCommandOpts): Promise<string> {
  const automaticallyIgnoredBuilds = await getAutomaticallyIgnoredBuilds(opts)
  let output = 'Automatically ignored builds:\n'
  if (automaticallyIgnoredBuilds == null) {
    output += '  Cannot identify as no node_modules found'
  } else if (automaticallyIgnoredBuilds.length === 0) {
    output += '  None'
  } else {
    output += `  ${automaticallyIgnoredBuilds.join('\n  ')}
hint: To allow the execution of build scripts for a package, add its name to "pnpm.onlyBuiltDependencies" in your "package.json", then run "pnpm rebuild".
hint: If you don't want to build a package, add it to the "pnpm.ignoredBuiltDependencies" list.`
  }
  output += '\n'
  if (opts.rootProjectManifest?.pnpm?.ignoredBuiltDependencies?.length) {
    output += `\nExplicitly ignored package builds (via pnpm.ignoredBuiltDependencies):\n  ${opts.rootProjectManifest.pnpm.ignoredBuiltDependencies.join('\n  ')}\n`
  }
  return output
}

async function getAutomaticallyIgnoredBuilds (opts: IgnoredBuildsCommandOpts) {
  const modulesManifest = await readModulesManifest(opts.modulesDir ?? path.join(opts.dir, 'node_modules'))
  if (modulesManifest == null) {
    return null
  }
  return modulesManifest?.ignoredBuilds ?? []
}
