/* eslint-disable no-await-in-loop */
import { PnpmError } from '@pnpm/error'
import { type NvmNodeCommandOptions } from './node'
import { downloadNodeVersion } from './downloadNodeVersion'

export async function envAdd (opts: NvmNodeCommandOptions, params: string[]) {
  if (!opts.global) {
    throw new PnpmError('NOT_IMPLEMENTED_YET', '"pnpm env use <version>" can only be used with the "--global" option currently')
  }
  const failed: string[] = []
  for (const envSpecifier of params) {
    const result = await downloadNodeVersion(opts, envSpecifier)
    if (!result) {
      failed.push(envSpecifier)
    }
  }
  if (failed.length > 0) {
    throw new PnpmError('COULD_NOT_RESOLVE_NODEJS', `Couldn't find Node.js version matching ${failed.join(', ')}`)
  }
  return 'All specified Node.js versions were installed'
}
