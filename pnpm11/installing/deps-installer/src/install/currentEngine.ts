import type { ProcessedInstallOptions as StrictInstallOptions } from './extendInstallOptions.js'

export function getCurrentEngine (
  opts: Pick<StrictInstallOptions, 'nodeVersion' | 'packageManager'>
): { nodeVersion: string | undefined, pnpmVersion: string } {
  return {
    nodeVersion: opts.nodeVersion,
    pnpmVersion: opts.packageManager.name === 'pnpm' ? opts.packageManager.version : '',
  }
}
