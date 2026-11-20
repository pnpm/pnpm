import type { CheckDepsStatusOptions } from './types.js'

/** The directory the store controller resolves a relative `storeDir` against. */
export function storeBaseDir (opts: Pick<CheckDepsStatusOptions, 'dir' | 'workspaceDir'>, stateDir: string): string {
  return opts.workspaceDir ?? opts.dir ?? stateDir
}
