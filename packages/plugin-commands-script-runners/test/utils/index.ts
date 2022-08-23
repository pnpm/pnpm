import path from 'path'
import { tempDir } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'

export const REGISTRY_URL = `http://localhost:${REGISTRY_MOCK_PORT}`
const tmp = tempDir()

export const DEFAULT_OPTS = {
  alwaysAuth: false,
  argv: {
    original: [],
  },
  bail: false,
  ca: undefined,
  cacheDir: '../cache',
  cert: undefined,
  extraEnv: {},
  cliOptions: {},
  extraBinPaths: [],
  fetchRetries: 2,
  fetchRetryFactor: 90,
  fetchRetryMaxtimeout: 90,
  fetchRetryMintimeout: 10,
  filter: [] as string[],
  httpsProxy: undefined,
  include: {
    dependencies: true,
    devDependencies: true,
    optionalDependencies: true,
  },
  key: undefined,
  linkWorkspacePackages: true,
  localAddress: undefined,
  lock: false,
  lockStaleDuration: 90,
  networkConcurrency: 16,
  offline: false,
  pending: false,
  pnpmfile: './.pnpmfile.cjs',
  pnpmHomeDir: '',
  proxy: undefined,
  rawConfig: { registry: REGISTRY_URL },
  rawLocalConfig: {},
  registries: { default: REGISTRY_URL },
  registry: REGISTRY_URL,
  sort: true,
  storeDir: '../store',
  strictSsl: false,
  userAgent: 'pnpm',
  useRunningStoreServer: false,
  useStoreServer: false,
  workspaceConcurrency: 4,
}

export const DLX_DEFAULT_OPTS = {
  argv: {
    original: [],
  },
  bail: false,
  bin: 'node_modules/.bin',
  cacheDir: path.join(tmp, 'cache'),
  extraEnv: {},
  cliOptions: {},
  include: {
    dependencies: true,
    devDependencies: true,
    optionalDependencies: true,
  },
  linkWorkspacePackages: true,
  lock: true,
  pnpmfile: '.pnpmfile.cjs',
  pnpmHomeDir: '',
  rawConfig: { registry: REGISTRY_URL },
  rawLocalConfig: { registry: REGISTRY_URL },
  registries: {
    default: REGISTRY_URL,
  },
  sort: true,
  storeDir: path.join(tmp, 'store'),
  userConfig: {},
  workspaceConcurrency: 1,
}
