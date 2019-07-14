import runLifecycleHooks from '@pnpm/lifecycle'
import { ImporterManifest } from '@pnpm/types'
import { realNodeModulesDir } from '@pnpm/utils'
import R = require('ramda')
import { readImporterManifestOnly } from '../readImporterManifest'

export default async function run (
  args: string[],
  opts: {
    extraBinPaths: string[],
    localPrefix: string,
    rawNpmConfig: object,
  },
) {
  const prefix = opts.localPrefix
  const manifest = await readImporterManifestOnly(prefix)
  const scriptName = args[0]
  if (!scriptName) {
    printProjectCommands(manifest)
    return
  }
  if (scriptName !== 'start' && (!manifest.scripts || !manifest.scripts[scriptName])) {
    const err = new Error(`Missing script: ${scriptName}`)
    err['code'] = 'ERR_PNPM_NO_SCRIPT'
    throw err
  }
  const lifecycleOpts = {
    args: args.slice(1),
    depPath: prefix,
    extraBinPaths: opts.extraBinPaths,
    pkgRoot: prefix,
    rawNpmConfig: opts.rawNpmConfig,
    rootNodeModulesDir: await realNodeModulesDir(prefix),
    stdio: 'inherit',
    unsafePerm: true, // when running scripts explicitly, assume that they're trusted.
  }
  if (manifest.scripts && manifest.scripts[`pre${scriptName}`]) {
    await runLifecycleHooks(`pre${scriptName}`, manifest, lifecycleOpts)
  }
  await runLifecycleHooks(scriptName, manifest, lifecycleOpts)
  if (manifest.scripts && manifest.scripts[`post${scriptName}`]) {
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

export async function start (
  args: string[],
  opts: {
    extraBinPaths: string[],
    localPrefix: string,
    rawNpmConfig: object,
    argv: {
      cooked: string[],
      original: string[],
      remain: string[],
    },
  },
) {
  return run(['start', ...args], opts)
}

export async function stop (
  args: string[],
  opts: {
    extraBinPaths: string[],
    localPrefix: string,
    rawNpmConfig: object,
    argv: {
      cooked: string[],
      original: string[],
      remain: string[],
    },
  },
) {
  return run(['stop', ...args], opts)
}

export async function test (
  args: string[],
  opts: {
    extraBinPaths: string[],
    localPrefix: string,
    rawNpmConfig: object,
    argv: {
      cooked: string[],
      original: string[],
      remain: string[],
    },
  },
) {
  return run(['test', ...args], opts)
}

export async function restart (
  args: string[],
  opts: {
    extraBinPaths: string[],
    localPrefix: string,
    rawNpmConfig: object,
    argv: {
      cooked: string[],
      original: string[],
      remain: string[],
    },
  },
) {
  await stop(args, opts)
  await run(['restart', ...args], opts)
  await start(args, opts)
}
