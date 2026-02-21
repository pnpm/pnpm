/* eslint-disable no-await-in-loop */
import { PnpmError } from '@pnpm/error'
import { type DownloadNodeVersionResult, downloadNodeVersion } from './downloadNodeVersion.js'
import { type NvmNodeCommandOptions } from './node.js'

export async function envAdd (opts: NvmNodeCommandOptions, params: string[]): Promise<string> {
  if (!opts.global) {
    throw new PnpmError('NOT_IMPLEMENTED_YET', '"pnpm env add <version>" can only be used with the "--global" option currently')
  }
  const installed: DownloadNodeVersionResult[] = []
  const failed: string[] = []
  for (const envSpecifier of params) {
    const result = await downloadNodeVersion(opts, envSpecifier)
    if (!result) {
      failed.push(envSpecifier)
    } else {
      installed.push(result)
    }
  }
  if (failed.length > 0) {
    throw new PnpmError('COULD_NOT_RESOLVE_NODEJS', `Couldn't find Node.js version matching ${failed.join(', ')}`)
  }
  if (opts.json) {
    return JSON.stringify(installed.map(({ nodeVersion, nodeDir }) => ({ version: nodeVersion, dir: nodeDir })))
  }
  return 'All specified Node.js versions were installed'
}
