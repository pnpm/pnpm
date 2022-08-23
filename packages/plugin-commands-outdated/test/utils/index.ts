import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'

const REGISTRY = `http://localhost:${REGISTRY_MOCK_PORT}`

export const DEFAULT_OPTS = {
  alwaysAuth: false,
  argv: {
    original: [],
  },
  bail: false,
  bin: 'node_modules/.bin',
  ca: undefined,
  cacheDir: '../cache',
  cert: undefined,
  extraEnv: {},
  cliOptions: {},
  fetchRetries: 2,
  fetchRetryFactor: 90,
  fetchRetryMaxtimeout: 90,
  fetchRetryMintimeout: 10,
  filter: [] as string[],
  global: false,
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
  rawConfig: { registry: REGISTRY },
  rawLocalConfig: {},
  registries: { default: REGISTRY },
  registry: REGISTRY,
  sort: true,
  storeDir: '../store',
  strictSsl: false,
  tag: 'latest',
  userAgent: 'pnpm',
  userConfig: {},
  useRunningStoreServer: false,
  useStoreServer: false,
  workspaceConcurrency: 4,
}
