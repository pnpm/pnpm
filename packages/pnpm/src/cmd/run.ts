import { docsUrl, readImporterManifestOnly } from '@pnpm/cli-utils'
import { FILTERING } from '@pnpm/common-cli-options-help'
import { types as allTypes } from '@pnpm/config'
import PnpmError from '@pnpm/error'
import runLifecycleHooks from '@pnpm/lifecycle'
import { ImporterManifest } from '@pnpm/types'
import { realNodeModulesDir } from '@pnpm/utils'
import { oneLine } from 'common-tags'
import R = require('ramda')
import renderHelp = require('render-help')

export function types () {
  return R.pick([
    'recursive',
  ], allTypes)
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
        ],
      },
      FILTERING,
    ],
    url: docsUrl('run'),
    usages: ['pnpm run <command> [-- <args>...]'],
  })
}

export async function handler (
  args: string[],
  opts: {
    engineStrict?: boolean,
    extraBinPaths: string[],
    dir: string,
    rawConfig: object,
  },
) {
  const dir = opts.dir
  const manifest = await readImporterManifestOnly(dir, opts)
  const scriptName = args[0]
  if (!scriptName) {
    printProjectCommands(manifest)
    return
  }
  if (scriptName !== 'start' && !manifest.scripts?.[scriptName]) {
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
  await runLifecycleHooks(scriptName, manifest, { ...lifecycleOpts, args: args.slice(1) })
  if (manifest.scripts?.[`post${scriptName}`]) {
    await runLifecycleHooks(`post${scriptName}`, manifest, lifecycleOpts)
  }
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

function printProjectCommands (manifest: ImporterManifest) {
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
    console.log(`There are no scripts specified.`)
    return
  }

  let output = ''
  if (lifecycleScripts.length > 0) {
    output += `Lifecycle scripts:\n${renderCommands(lifecycleScripts)}`
  }
  if (otherScripts.length > 0) {
    if (output !== '') output += '\n\n'
    output += `Commands available via "pnpm run":\n${renderCommands(otherScripts)}`
  }
  console.log(output)
}

function renderCommands (commands: string[][]) {
  return commands.map(([scriptName, script]) => `  ${scriptName}\n    ${script}`).join('\n')
}
