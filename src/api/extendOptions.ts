import {StrictPnpmOptions, PnpmOptions} from '../types'

const defaults = () => (<StrictPnpmOptions>{
  fetchRetries: 2,
  fetchRetryFactor: 10,
  fetchRetryMintimeout: 1e4, // 10 seconds
  fetchRetryMaxtimeout: 6e4, // 1 minute
  storePath: 'node_modules/.store',
  globalPath: '~/.pnpm',
  logger: 'pretty',
  ignoreScripts: false,
  linkLocal: false,
  strictSsl: true,
  tag: 'latest',
  production: process.env.NODE_ENV === 'production',
  cwd: process.cwd(),
  force: false,
  silent: true,
  depth: 0
})

export default (opts?: PnpmOptions): StrictPnpmOptions => {
  return Object.assign({}, defaults(), opts)
}
