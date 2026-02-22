import { PnpmError } from '@pnpm/error'
import { runPnpmCli } from '@pnpm/exec.pnpm-cli-runner'
import { type NvmNodeCommandOptions } from './node.js'

export async function envUse (opts: NvmNodeCommandOptions, params: string[]): Promise<string> {
  if (!opts.global) {
    throw new PnpmError('NOT_IMPLEMENTED_YET', '"pnpm env use <version>" can only be used with the "--global" option currently')
  }

  const args = ['add', '--global', `node@runtime:${params[0]}`]
  if (opts.bin) args.push('--global-bin-dir', opts.bin)
  if (opts.storeDir) args.push('--store-dir', opts.storeDir)
  if (opts.cacheDir) args.push('--cache-dir', opts.cacheDir)
  runPnpmCli(args, { cwd: opts.pnpmHomeDir })

  return `Node.js ${params[0]} was activated`
}
