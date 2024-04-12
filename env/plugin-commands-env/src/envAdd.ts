/* eslint-disable no-await-in-loop */
import { PnpmError } from '@pnpm/error'
import { downloadNodeVersion } from './downloadNodeVersion'
import { type NvmNodeCommandOptions } from './node'

export async function envAdd (opts: NvmNodeCommandOptions, params: string[]): Promise<string> {
  if (!opts.global) {
    throw new PnpmError('NOT_IMPLEMENTED_YET', '"pnpm env add <version>" can only be used with the "--global" option currently')
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
