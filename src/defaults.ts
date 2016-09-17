import {StrictPublicInstallationOptions} from './api/install'

export default <StrictPublicInstallationOptions>{
  concurrency: 16,
  fetchRetries: 2,
  fetchRetryFactor: 10,
  fetchRetryMintimeout: 1e4, // 10 seconds
  fetchRetryMaxtimeout: 6e4, // 1 minute
  storePath: 'node_modules/.store',
  globalPath: '~/.pnpm',
  logger: 'pretty',
  ignoreScripts: false,
  linkLocal: false
}
