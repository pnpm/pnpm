import { PnpmError } from '@pnpm/error'
import diable from '@zkochan/diable'

export default (storePath: string) => {
  if (require.main == null) {
    throw new PnpmError('CANNOT_START_SERVER', 'pnpm server cannot be started when pnpm is streamed to Node.js')
  }
  return diable.daemonize(require.main.filename, ['server', 'start', '--store-dir', storePath], { stdio: 'inherit' })
}
