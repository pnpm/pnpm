import logger from 'pnpm-logger'
import {PnpmOptions, uninstall} from 'supi'

export default function uninstallCmd (input: string[], opts: PnpmOptions, cmdName: string) {
  if (cmdName === 'unlink') {
    logger.warn('This command will behave as `pnpm dislink` in the future')
  }
  return uninstall(input, opts)
}
