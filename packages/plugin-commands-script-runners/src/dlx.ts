import fs from 'fs'
import path from 'path'
import storePath from '@pnpm/store-path'
import rimraf from '@zkochan/rimraf'
import execa from 'execa'
import PATH from 'path-name'
import renderHelp from 'render-help'

export const commandNames = ['dlx']

export function rcOptionsTypes () {
  return {}
}

export const cliOptionsTypes = () => ({
  package: String,
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
    ],
    usages: ['pnpm dlx <command> [args...]'],
  })
}

export async function handler (
  opts: {
    package?: string
  },
  params: string[]
) {
  const cache = path.join(await storePath(process.cwd(), '~/.pnpm-store'), 'tmp')
  const prefix = path.join(cache, `dlx-${process.pid.toString()}`)
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
    } catch (err) { }
  })
  await rimraf(bins)
  const pkg = opts.package ?? params[0]
  await execa('pnpm', ['add', pkg, '--global', '--global-dir', prefix, '--dir', prefix], {
    stdio: 'inherit',
  })
  await execa(params[0], params.slice(1), {
    env: {
      ...process.env,
      [PATH]: [
        bins,
        process.env[PATH],
      ].join(path.delimiter),
    },
    stdio: 'inherit',
  })
}
