import logger from '@pnpm/logger'
import { PnpmOptions } from '../../types'
import help from '../help'
import start from './start'
import stop from './stop'

export default async (
  input: string[],
  opts: PnpmOptions & {
    protocol?: 'auto' | 'tcp' | 'ipc',
    port?: number,
    unstoppable?: boolean,
  },
) => {
  if (input[0]) {
    logger.warn('The store server is an experimental feature. Breaking changes may happen in non-major versions.')
  }

  switch (input[0]) {
    case 'start':
      return start(opts)
    case 'stop':
      return stop(opts)
    default:
      help(['server'])
  }
}
