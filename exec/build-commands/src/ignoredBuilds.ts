import { type Config } from '@pnpm/config'
import renderHelp from 'render-help'
import { getAutomaticallyIgnoredBuilds } from './getAutomaticallyIgnoredBuilds.js'

export type IgnoredBuildsCommandOpts = Pick<Config, 'modulesDir' | 'dir' | 'allowBuilds' | 'lockfileDir'>

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
  const disallowedBuilds = opts.allowBuilds
    ? Object.entries(opts.allowBuilds)
      .filter(([, value]) => value === false)
      .map(([pkg]) => pkg)
    : []
  let { automaticallyIgnoredBuilds } = await getAutomaticallyIgnoredBuilds(opts)
  if (automaticallyIgnoredBuilds) {
    automaticallyIgnoredBuilds = automaticallyIgnoredBuilds
      .filter((automaticallyIgnoredBuild) => !disallowedBuilds.includes(automaticallyIgnoredBuild))
  }
  let output = 'Automatically ignored builds during installation:\n'
  if (automaticallyIgnoredBuilds == null) {
    output += '  Cannot identify as no node_modules found'
  } else if (automaticallyIgnoredBuilds.length === 0) {
    output += '  None'
  } else {
    output += `  ${automaticallyIgnoredBuilds.join('\n  ')}
hint: To allow the execution of build scripts for a package, add its name to "allowBuilds" and set to "true", then run "pnpm rebuild".
hint: For example:
hint: allowBuilds:
hint:   esbuild: true
hint: If you don't want to build a package, set it to "false" instead.`
  }
  output += '\n'
  if (disallowedBuilds.length) {
    output += `\nExplicitly ignored package builds (via allowBuilds):\n  ${disallowedBuilds.join('\n  ')}\n`
  }
  return output
}
