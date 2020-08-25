import { types as allTypes, UniversalOptions } from '@pnpm/config'
import runNpm from '@pnpm/run-npm'
import { fakeRegularManifest } from './publish'
import R = require('ramda')
import renderHelp = require('render-help')

export function rcOptionsTypes () {
  return {
    ...cliOptionsTypes(),
    ...R.pick([
      'npm-path',
    ], allTypes),
  }
}

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
  opts: Pick<UniversalOptions, 'dir'> & {
    argv: {
      original: string[]
    }
    engineStrict?: boolean
    npmPath?: string
    workspaceDir?: string
  }
) {
  let _status!: number
  await fakeRegularManifest({
    dir: opts.dir,
    engineStrict: opts.engineStrict,
    workspaceDir: opts.workspaceDir ?? opts.dir,
  }, async () => {
    const { status } = await runNpm(opts.npmPath, ['pack', ...opts.argv.original.slice(1)])
    _status = status!
  })
  if (_status !== 0) {
    process.exit(_status)
  }
}
