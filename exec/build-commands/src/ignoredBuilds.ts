import { type Config } from '@pnpm/config'
import renderHelp from 'render-help'
import { getAutomaticallyIgnoredBuilds } from './getAutomaticallyIgnoredBuilds'

export type IgnoredBuildsCommandOpts = Pick<Config, 'modulesDir' | 'dir' | 'rootProjectManifest' | 'lockfileDir'>

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
  const ignoredBuiltDependencies = opts.rootProjectManifest?.pnpm?.ignoredBuiltDependencies ?? []
  let { automaticallyIgnoredBuilds } = await getAutomaticallyIgnoredBuilds(opts)
  if (automaticallyIgnoredBuilds) {
    automaticallyIgnoredBuilds = automaticallyIgnoredBuilds
      .filter((automaticallyIgnoredBuild) => !ignoredBuiltDependencies.includes(automaticallyIgnoredBuild))
  }
  let output = 'Automatically ignored builds during installation:\n'
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
  if (ignoredBuiltDependencies.length) {
    output += `\nExplicitly ignored package builds (via pnpm.ignoredBuiltDependencies):\n  ${ignoredBuiltDependencies.join('\n  ')}\n`
  }
  return output
}
