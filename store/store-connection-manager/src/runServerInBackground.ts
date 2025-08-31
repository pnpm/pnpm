import { PnpmError } from '@pnpm/error'
// cspell:ignore diable
import diable from '@zkochan/diable'

export function runServerInBackground (storePath: string): void {
  const entry = process.argv[1]
  if (!entry) {
    throw new PnpmError('CANNOT_START_SERVER', 'pnpm server cannot be started when pnpm is streamed to Node.js')
  }
  return diable.daemonize(entry, ['server', 'start', '--store-dir', storePath], { stdio: 'inherit' })
}
