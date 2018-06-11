import logger from '@pnpm/logger'
import { PnpmOptions } from '../../types'
import help from '../help'
import start from './start'
import status from './status'
import stop from './stop'

export default async (
  input: string[],
  opts: PnpmOptions & {
    protocol?: 'auto' | 'tcp' | 'ipc',
    port?: number,
    unstoppable?: boolean,
  },
) => {
  switch (input[0]) {
    case 'start':
      return start(opts)
    case 'status':
      return status(opts)
    case 'stop':
      return stop(opts)
    default:
      help(['server'])
  }
}
