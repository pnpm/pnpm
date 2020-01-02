import { UniversalOptions } from '@pnpm/config'
import runNpm from '@pnpm/run-npm'
import renderHelp = require('render-help')
import { fakeRegularManifest } from './publish'

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes () {
  return {}
}

export const commandNames = ['pack']

export function help () {
  return renderHelp({
    description: 'Creates a compressed gzip archive of package dependencies.',
    usages: ['pnpm pack'],
  })
}

export async function handler (
  args: string[],
  opts: Pick<UniversalOptions, 'dir'> & {
    argv: {
      original: string[],
    },
    engineStrict?: boolean,
    workspaceDir?: string,
  },
) {
  let _status!: number
  await fakeRegularManifest({
    dir: opts.dir,
    engineStrict: opts.engineStrict,
    workspaceDir: opts.workspaceDir || opts.dir,
  }, async () => {
    const { status } = await runNpm(['pack', ...opts.argv.original.slice(1)])
    _status = status!
  })
  if (_status !== 0) {
    process.exit(_status)
  }
}
