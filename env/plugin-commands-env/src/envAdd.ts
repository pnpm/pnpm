/* eslint-disable no-await-in-loop */
import { PnpmError } from '@pnpm/error'
import { type NvmNodeCommandOptions } from './node'
import { downloadNodeVersion } from './downloadNodeVersion'
import { globalWarn } from '@pnpm/logger'

export async function envAdd (opts: NvmNodeCommandOptions, params: string[]) {
  if (!opts.global) {
    throw new PnpmError('NOT_IMPLEMENTED_YET', '"pnpm env use <version>" can only be used with the "--global" option currently')
  }
  const errors = []
  for (const version of params) {
    const message = await downloadNodeVersion(opts, version)
    if (message instanceof Error) {
      globalWarn(message.message)
      errors.push(message)
    }
  }
  if (errors.length > 0) throw errors[0]
  return 'All specified Node.js versions were installed'
}
