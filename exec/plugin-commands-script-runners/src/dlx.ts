import fs from 'fs'
import path from 'path'
import { docsUrl } from '@pnpm/cli-utils'
import { OUTPUT_OPTIONS } from '@pnpm/common-cli-options-help'
import { type Config, types } from '@pnpm/config'
import { PnpmError } from '@pnpm/error'
import { add } from '@pnpm/plugin-commands-installation'
import { readPackageJsonFromDir } from '@pnpm/read-package-json'
import { getBinsFromPackageManifest } from '@pnpm/package-bins'
import { getStorePath } from '@pnpm/store-path'
import execa from 'execa'
import omit from 'ramda/src/omit'
import pick from 'ramda/src/pick'
import renderHelp from 'render-help'
import { makeEnv } from './makeEnv'

export const commandNames = ['dlx']

export const shorthands = {
  c: '--shell-mode',
}

export function rcOptionsTypes () {
  return {
    ...pick([
      'use-node-version',
    ], types),
    'shell-mode': Boolean,
  }
}

export const cliOptionsTypes = () => ({
  ...rcOptionsTypes(),
  package: [String, Array],
})

export function help () {
  return renderHelp({
    description: 'Run a package in a temporary environment.',
    descriptionLists: [
      {
        title: 'Options',
        list: [
          {
            description: 'The package to install before running the command',
            name: '--package',
          },
          {
            description: 'Runs the script inside of a shell. Uses /bin/sh on UNIX and \\cmd.exe on Windows.',
            name: '--shell-mode',
            shortAlias: '-c',
          },
        ],
      },
      OUTPUT_OPTIONS,
    ],
    url: docsUrl('dlx'),
    usages: ['pnpm dlx <command> [args...]'],
  })
}

export type DlxCommandOptions = {
  package?: string[]
  shellMode?: boolean
} & Pick<Config, 'reporter' | 'userAgent'> & add.AddCommandOptions

export async function handler (
  opts: DlxCommandOptions,
  [command, ...args]: string[]
) {
  const dlxDir = await getDlxDir({
    dir: opts.dir,
    pnpmHomeDir: opts.pnpmHomeDir,
    storeDir: opts.storeDir,
  })
  const prefix = path.join(dlxDir, `dlx-${process.pid.toString()}`)
  const modulesDir = path.join(prefix, 'node_modules')
  const binsDir = path.join(modulesDir, '.bin')
  fs.mkdirSync(prefix, { recursive: true })
  process.on('exit', () => {
    try {
      fs.rmdirSync(prefix, {
        recursive: true,
        maxRetries: 3,
      })
    } catch (err) {}
  })
  const pkgs = opts.package ?? [command]
  const env = makeEnv({ userAgent: opts.userAgent, prependPaths: [binsDir] })
  await add.handler({
    ...omit(['workspaceDir'], opts),
    bin: binsDir,
    dir: prefix,
    lockfileDir: prefix,
  }, pkgs)
  const binName = opts.package
    ? command
    : await getBinName(modulesDir, await getPkgName(prefix))
  try {
    await execa(binName, args, {
      cwd: process.cwd(),
      env,
      stdio: 'inherit',
      shell: opts.shellMode ?? false,
    })
  } catch (err: any) { // eslint-disable-line
    if (err.exitCode != null) {
      return {
        exitCode: err.exitCode,
      }
    }
    throw err
  }
  return { exitCode: 0 }
}

async function getPkgName (pkgDir: string) {
  const manifest = await readPackageJsonFromDir(pkgDir)
  return Object.keys(manifest.dependencies ?? {})[0]
}

async function getBinName (modulesDir: string, pkgName: string): Promise<string> {
  const pkgDir = path.join(modulesDir, pkgName)
  const manifest = await readPackageJsonFromDir(pkgDir)
  const bins = await getBinsFromPackageManifest(manifest, pkgDir)
  if (bins.length === 0) {
    throw new PnpmError('DLX_NO_BIN', `No binaries found in ${pkgName}`)
  }
  if (bins.length === 1) {
    return bins[0].name
  }
  const scopelessPkgName = scopeless(manifest.name)
  const defaultBin = bins.find(({ name }) => name === scopelessPkgName)
  if (defaultBin) return defaultBin.name
  const binNames = bins.map(({ name }) => name)
  throw new PnpmError('DLX_MULTIPLE_BINS', `Could not determine executable to run. ${pkgName} has multiple binaries: ${binNames.join(', ')}`, {
    hint: `Try one of the following:
${binNames.map(name => `pnpm --package=${pkgName} dlx ${name}`).join('\n')}
`,
  })
}

function scopeless (pkgName: string) {
  if (pkgName.startsWith('@')) {
    return pkgName.split('/')[1]
  }
  return pkgName
}

async function getDlxDir (
  opts: {
    dir: string
    storeDir?: string
    pnpmHomeDir: string
  }
): Promise<string> {
  const storeDir = await getStorePath({
    pkgRoot: opts.dir,
    storePath: opts.storeDir,
    pnpmHomeDir: opts.pnpmHomeDir,
  })
  return path.join(storeDir, 'tmp')
}
