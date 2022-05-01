import fs from 'fs'
import os from 'os'
import path from 'path'
import { docsUrl } from '@pnpm/cli-utils'
import { OUTPUT_OPTIONS } from '@pnpm/common-cli-options-help'
import { Config } from '@pnpm/config'
import rimraf from '@zkochan/rimraf'
import execa from 'execa'
import renderHelp from 'render-help'
import { makeEnv } from './makeEnv'

export const commandNames = ['dlx']

export function rcOptionsTypes () {
  return {}
}

export const cliOptionsTypes = () => ({
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
        ],
      },
      OUTPUT_OPTIONS,
    ],
    url: docsUrl('dlx'),
    usages: ['pnpm dlx <command> [args...]'],
  })
}

export async function handler (
  opts: {
    package?: string[]
  } & Pick<Config, 'reporter' | 'userAgent'>,
  params: string[]
) {
  const prefix = path.join(fs.realpathSync(os.tmpdir()), `dlx-${process.pid.toString()}`)
  const bins = process.platform === 'win32'
    ? prefix
    : path.join(prefix, 'bin')
  fs.mkdirSync(prefix, { recursive: true })
  process.on('exit', () => {
    try {
      fs.rmdirSync(prefix, {
        recursive: true,
        maxRetries: 3,
      })
    } catch (err) {}
  })
  await rimraf(bins)
  const pkgs = opts.package ?? params.slice(0, 1)
  const pnpmArgs = [
    'add',
    ...pkgs,
    '--global',
    `--global-dir=${prefix}`,
    `--dir=${prefix}`,
    `--config.global-bin-dir=${bins}`,
  ]
  if (opts.reporter) {
    pnpmArgs.push(`--reporter=${opts.reporter}`)
  }
  const env = makeEnv({ userAgent: opts.userAgent, prependPaths: [bins] })
  await execa('pnpm', pnpmArgs, {
    env,
    stdio: 'inherit',
  })
  await execa(versionless(scopeless(params[0])), params.slice(1), {
    env,
    stdio: 'inherit',
  })
}

function scopeless (pkgName: string) {
  if (pkgName.startsWith('@')) {
    return pkgName.split('/')[1]
  }
  return pkgName
}

function versionless (scopelessPkgName: string) {
  return scopelessPkgName.split('@')[0]
}
