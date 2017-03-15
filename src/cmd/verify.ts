import verify from '../api/verify'
import {PnpmOptions} from '../types'
import logger from 'pnpm-logger'

const verifyLogger = logger('verify')

export default async function (input: string[], opts: PnpmOptions) {
  const err = await verify(process.cwd())
  if (!err) return
  verifyLogger.error(err)
  if (!opts.exit0) process.exit(1)
}
