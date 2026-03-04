import { PnpmError } from '@pnpm/error'
import { runPnpmCli } from '@pnpm/exec.pnpm-cli-runner'
import { globalWarn } from '@pnpm/logger'
import { type NvmNodeCommandOptions } from './node.js'

export async function envUse (opts: NvmNodeCommandOptions, params: string[]): Promise<void> {
  globalWarn('"pnpm env use" is deprecated. Use "pnpm runtime set node <version> -g" instead.')
  if (!opts.global) {
    throw new PnpmError('NOT_IMPLEMENTED_YET', '"pnpm env use <version>" can only be used with the "--global" option currently')
  }

  const version = params[0]?.trim()
  if (!version) {
    throw new PnpmError('MISSING_NODE_VERSION', '"pnpm env use --global <version>" requires a Node.js version to be specified')
  }

  const args = ['add', '--global', `node@runtime:${version}`]
  if (opts.bin) args.push('--global-bin-dir', opts.bin)
  if (opts.storeDir) args.push('--store-dir', opts.storeDir)
  if (opts.cacheDir) args.push('--cache-dir', opts.cacheDir)
  runPnpmCli(args, { cwd: opts.pnpmHomeDir })
}
