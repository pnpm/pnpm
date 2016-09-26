import {StrictBasicOptions} from './api/initCmd' // tslint:disable-line
import {StrictPublicInstallationOptions} from './api/install'

export default <StrictPublicInstallationOptions>{
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
  tag: 'latest'
}
