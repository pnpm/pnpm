import path from 'path'
import {
  docsUrl,
  readProjectManifestOnly,
  tryReadProjectManifest,
} from '@pnpm/cli-utils'
import { CompletionFunc } from '@pnpm/command'
import { FILTERING, UNIVERSAL_OPTIONS } from '@pnpm/common-cli-options-help'
import { Config, types as allTypes } from '@pnpm/config'
import PnpmError from '@pnpm/error'
import runLifecycleHooks, {
  makeNodeRequireOption,
  RunLifecycleHookOptions,
} from '@pnpm/lifecycle'
import { ProjectManifest } from '@pnpm/types'
import pick from 'ramda/src/pick'
import realpathMissing from 'realpath-missing'
import renderHelp from 'render-help'
import runRecursive, { RecursiveRunOpts } from './runRecursive'
import existsInDir from './existsInDir'
import { handler as exec } from './exec'

export const IF_PRESENT_OPTION = {
  'if-present': Boolean,
}

export const IF_PRESENT_OPTION_HELP = {
  description: 'Avoid exiting with a non-zero exit code when the script is undefined',
  name: '--if-present',
}

export const PARALLEL_OPTION_HELP = {
  description: 'Completely disregard concurrency and topological sorting, \
running a given script immediately in all matching packages \
with prefixed streaming output. This is the preferred flag \
for long-running processes such as watch run over many packages.',
  name: '--parallel',
}

export const shorthands = {
  parallel: [
    '--workspace-concurrency=Infinity',
    '--no-sort',
    '--stream',
    '--recursive',
  ],
}

export function rcOptionsTypes () {
  return {
    ...pick([
      'npm-path',
    ], allTypes),
  }
}

export function cliOptionsTypes () {
  return {
    ...pick([
      'bail',
      'sort',
      'unsafe-perm',
      'workspace-concurrency',
      'scripts-prepend-node-path',
    ], allTypes),
    ...IF_PRESENT_OPTION,
    recursive: Boolean,
    reverse: Boolean,
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
            description: 'Run the defined package script in every package found in subdirectories \
or every workspace package, when executed inside a workspace. \
For options that may be used with `-r`, see "pnpm help recursive"',
            name: '--recursive',
            shortAlias: '-r',
          },
          {
            description: 'The command will exit with a 0 exit code even if the script fails',
            name: '--no-bail',
          },
          IF_PRESENT_OPTION_HELP,
          PARALLEL_OPTION_HELP,
          ...UNIVERSAL_OPTIONS,
        ],
      },
      FILTERING,
    ],
    url: docsUrl('run'),
    usages: ['pnpm run <command> [<args>...]'],
  })
}

export type RunOpts =
  & Omit<RecursiveRunOpts, 'allProjects' | 'selectedProjectsGraph' | 'workspaceDir'>
  & { recursive?: boolean }
  & Pick<Config, 'dir' | 'engineStrict' | 'extraBinPaths' | 'reporter' | 'scriptsPrependNodePath' | 'scriptShell' | 'shellEmulator' | 'enablePrePostScripts'>
  & (
    & { recursive?: false }
    & Partial<Pick<Config, 'allProjects' | 'selectedProjectsGraph' | 'workspaceDir'>>
    | { recursive: true }
    & Required<Pick<Config, 'allProjects' | 'selectedProjectsGraph' | 'workspaceDir'>>
  )
  & {
    argv?: {
      original: string[]
    }
    fallbackCommandUsed?: boolean
  }

export async function handler (
  opts: RunOpts,
  params: string[]
) {
  let dir: string
  const [scriptName, ...passedThruArgs] = params
  // For backward compatibility
  const firstDoubleDash = passedThruArgs.findIndex((arg) => arg === '--')
  if (firstDoubleDash !== -1) {
    passedThruArgs.splice(firstDoubleDash, 1)
  }
  if (opts.recursive) {
    if (scriptName || Object.keys(opts.selectedProjectsGraph).length > 1) {
      return runRecursive(params, opts)
    }
    dir = Object.keys(opts.selectedProjectsGraph)[0]
  } else {
    dir = opts.dir
  }
  const manifest = await readProjectManifestOnly(dir, opts)
  if (!scriptName) {
    const rootManifest = opts.workspaceDir && opts.workspaceDir !== dir
      ? (await tryReadProjectManifest(opts.workspaceDir, opts)).manifest
      : undefined
    return printProjectCommands(manifest, rootManifest ?? undefined)
  }
  if (scriptName !== 'start' && !manifest.scripts?.[scriptName]) {
    if (opts.ifPresent) return
    if (opts.fallbackCommandUsed) {
      if (opts.argv == null) throw new Error('Could not fallback because opts.argv.original was not passed to the script runner')
      return exec({
        selectedProjectsGraph: {},
        ...opts,
      }, opts.argv.original.slice(1))
    }
    if (opts.workspaceDir) {
      const { manifest: rootManifest } = await tryReadProjectManifest(opts.workspaceDir, opts)
      if (rootManifest?.scripts?.[scriptName]) {
        throw new PnpmError('NO_SCRIPT', `Missing script: ${scriptName}`, {
          hint: `But ${scriptName} is present in the root of the workspace,
so you may run "pnpm -w run ${scriptName}"`,
        })
      }
    }
    throw new PnpmError('NO_SCRIPT', `Missing script: ${scriptName}`)
  }
  const lifecycleOpts: RunLifecycleHookOptions = {
    depPath: dir,
    extraBinPaths: opts.extraBinPaths,
    pkgRoot: dir,
    rawConfig: opts.rawConfig,
    rootModulesDir: await realpathMissing(path.join(dir, 'node_modules')),
    scriptsPrependNodePath: opts.scriptsPrependNodePath,
    scriptShell: opts.scriptShell,
    silent: opts.reporter === 'silent',
    shellEmulator: opts.shellEmulator,
    stdio: 'inherit',
    unsafePerm: true, // when running scripts explicitly, assume that they're trusted.
  }
  const existsPnp = existsInDir.bind(null, '.pnp.cjs')
  const pnpPath = (opts.workspaceDir && await existsPnp(opts.workspaceDir)) ??
    await existsPnp(dir)
  if (pnpPath) {
    lifecycleOpts.extraEnv = makeNodeRequireOption(pnpPath)
  }
  try {
    if (
      opts.enablePrePostScripts &&
      manifest.scripts?.[`pre${scriptName}`] &&
      !manifest.scripts[scriptName].includes(`pre${scriptName}`)
    ) {
      await runLifecycleHooks(`pre${scriptName}`, manifest, lifecycleOpts)
    }
    await runLifecycleHooks(scriptName, manifest, { ...lifecycleOpts, args: passedThruArgs })
    if (
      opts.enablePrePostScripts &&
      manifest.scripts?.[`post${scriptName}`] &&
      !manifest.scripts[scriptName].includes(`post${scriptName}`)
    ) {
      await runLifecycleHooks(`post${scriptName}`, manifest, lifecycleOpts)
    }
  } catch (err: any) { // eslint-disable-line
    if (opts.bail !== false) {
      throw err
    }
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

function printProjectCommands (
  manifest: ProjectManifest,
  rootManifest?: ProjectManifest
) {
  const lifecycleScripts = [] as string[][]
  const otherScripts = [] as string[][]

  for (const [scriptName, script] of Object.entries(manifest.scripts ?? {})) {
    if (ALL_LIFECYCLE_SCRIPTS.has(scriptName)) {
      lifecycleScripts.push([scriptName, script])
    } else {
      otherScripts.push([scriptName, script])
    }
  }

  if (lifecycleScripts.length === 0 && otherScripts.length === 0) {
    return 'There are no scripts specified.'
  }

  let output = ''
  if (lifecycleScripts.length > 0) {
    output += `Lifecycle scripts:\n${renderCommands(lifecycleScripts)}`
  }
  if (otherScripts.length > 0) {
    if (output !== '') output += '\n\n'
    output += `Commands available via "pnpm run":\n${renderCommands(otherScripts)}`
  }
  if ((rootManifest?.scripts) == null) {
    return output
  }
  const rootScripts = Object.entries(rootManifest.scripts)
  if (rootScripts.length === 0) {
    return output
  }
  if (output !== '') output += '\n\n'
  output += `Commands of the root workspace project (to run them, use "pnpm -w run"):
${renderCommands(rootScripts)}`
  return output
}

function renderCommands (commands: string[][]) {
  return commands.map(([scriptName, script]) => `  ${scriptName}\n    ${script}`).join('\n')
}
