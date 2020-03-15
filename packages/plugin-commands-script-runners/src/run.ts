import { docsUrl, readProjectManifestOnly } from '@pnpm/cli-utils'
import { CompletionFunc } from '@pnpm/command'
import { FILTERING } from '@pnpm/common-cli-options-help'
import { Config, types as allTypes } from '@pnpm/config'
import PnpmError from '@pnpm/error'
import runLifecycleHooks from '@pnpm/lifecycle'
import { ProjectManifest } from '@pnpm/types'
import { realNodeModulesDir } from '@pnpm/utils'
import { oneLine } from 'common-tags'
import R = require('ramda')
import renderHelp = require('render-help')
import runRecursive, { RecursiveRunOpts } from './runRecursive'

export const IF_PRESENT_OPTION = {
  'if-present': Boolean,
}

export const IF_PRESENT_OPTION_HELP = {
  description: 'Avoid exiting with a non-zero exit code when the script is undefined',
  name: '--if-present',
}

export function rcOptionsTypes () {
  return {
    ...R.pick([
      'npm-path',
    ], allTypes),
  }
}

export function cliOptionsTypes () {
  return {
    ...R.pick([
      'bail',
      'sort',
      'unsafe-perm',
      'workspace-concurrency',
    ], allTypes),
    ...IF_PRESENT_OPTION,
    recursive: Boolean,
  }
}

export const completion: CompletionFunc = async (cliOpts, params) => {
  if (params.length > 0) {
    return []
  }
  const manifest = await readProjectManifestOnly(cliOpts.dir as string ?? process.cwd(), cliOpts)
  return Object.keys(manifest.scripts ?? {}).map((name) => ({ name }))
}

export const commandNames = ['run', 'run-script']

export function help () {
  return renderHelp({
    aliases: ['run-script'],
    description: 'Runs a defined package script.',
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description: oneLine`Run the defined package script in every package found in subdirectories
              or every workspace package, when executed inside a workspace.
              For options that may be used with \`-r\`, see "pnpm help recursive"`,
            name: '--recursive',
            shortAlias: '-r',
          },
          IF_PRESENT_OPTION_HELP,
        ],
      },
      FILTERING,
    ],
    url: docsUrl('run'),
    usages: ['pnpm run <command> [-- <args>...]'],
  })
}

export type RunOpts = Omit<RecursiveRunOpts, 'allProjects' | 'selectedProjectsGraph' | 'workspaceDir'> & {
  ifPresent?: boolean,
  recursive?: boolean,
} & Pick<Config, 'dir' | 'engineStrict'> & (
  { recursive?: false } &
  Partial<Pick<Config, 'allProjects' | 'selectedProjectsGraph' | 'workspaceDir'>>
  |
  { recursive: true } &
  Required<Pick<Config, 'allProjects' | 'selectedProjectsGraph' | 'workspaceDir'>>
)

export async function handler (
  opts: RunOpts,
  params: string[],
) {
  let dir: string
  const [scriptName, ...passedThruArgs] = params
  if (opts.recursive) {
    if (scriptName || Object.keys(opts.selectedProjectsGraph).length > 1) {
      await runRecursive(params, opts)
      return
    }
    dir = Object.keys(opts.selectedProjectsGraph)[0]
  } else {
    dir = opts.dir
  }
  const manifest = await readProjectManifestOnly(dir, opts)
  if (!scriptName) {
    return printProjectCommands(manifest)
  }
  if (scriptName !== 'start' && !manifest.scripts?.[scriptName]) {
    if (opts.ifPresent) return
    throw new PnpmError('NO_SCRIPT', `Missing script: ${scriptName}`)
  }
  const lifecycleOpts = {
    depPath: dir,
    extraBinPaths: opts.extraBinPaths,
    pkgRoot: dir,
    rawConfig: opts.rawConfig,
    rootNodeModulesDir: await realNodeModulesDir(dir),
    stdio: 'inherit',
    unsafePerm: true, // when running scripts explicitly, assume that they're trusted.
  }
  if (manifest.scripts?.[`pre${scriptName}`]) {
    await runLifecycleHooks(`pre${scriptName}`, manifest, lifecycleOpts)
  }
  await runLifecycleHooks(scriptName, manifest, { ...lifecycleOpts, args: passedThruArgs })
  if (manifest.scripts?.[`post${scriptName}`]) {
    await runLifecycleHooks(`post${scriptName}`, manifest, lifecycleOpts)
  }
  return undefined
}

const ALL_LIFECYCLE_SCRIPTS = new Set([
  'prepublish',
  'prepare',
  'prepublishOnly',
  'prepack',
  'postpack',
  'publish',
  'postpublish',
  'preinstall',
  'install',
  'postinstall',
  'preuninstall',
  'uninstall',
  'postuninstall',
  'preversion',
  'version',
  'postversion',
  'pretest',
  'test',
  'posttest',
  'prestop',
  'stop',
  'poststop',
  'prestart',
  'start',
  'poststart',
  'prerestart',
  'restart',
  'postrestart',
  'preshrinkwrap',
  'shrinkwrap',
  'postshrinkwrap',
])

function printProjectCommands (manifest: ProjectManifest) {
  const lifecycleScripts = [] as string[][]
  const otherScripts = [] as string[][]

  for (const [scriptName, script] of R.toPairs(manifest.scripts || {})) {
    if (ALL_LIFECYCLE_SCRIPTS.has(scriptName)) {
      lifecycleScripts.push([scriptName, script])
    } else {
      otherScripts.push([scriptName, script])
    }
  }

  if (lifecycleScripts.length === 0 && otherScripts.length === 0) {
    return `There are no scripts specified.`
  }

  let output = ''
  if (lifecycleScripts.length > 0) {
    output += `Lifecycle scripts:\n${renderCommands(lifecycleScripts)}`
  }
  if (otherScripts.length > 0) {
    if (output !== '') output += '\n\n'
    output += `Commands available via "pnpm run":\n${renderCommands(otherScripts)}`
  }
  return output
}

function renderCommands (commands: string[][]) {
  return commands.map(([scriptName, script]) => `  ${scriptName}\n    ${script}`).join('\n')
}
