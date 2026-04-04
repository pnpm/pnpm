import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'

const registries = {
  default: 'https://registry.npmjs.org/',
}
const authConfig = {
  registry: registries.default,
}
export const DEFAULT_OPTS = {
  argv: {
    original: [],
  },
  bail: true,
  bin: 'node_modules/.bin',
  ca: undefined,
  cacheDir: '../cache',
  cert: undefined,
  excludeLinksFromLockfile: false,
  extraEnv: {},
  cliOptions: {},
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
  minimumReleaseAge: 0,
  networkConcurrency: 16,
  offline: false,
  pending: false,
  pnpmfile: ['./.pnpmfile.cjs'],
  pnpmHomeDir: '',
  preferWorkspacePackages: true,
  proxy: undefined,
  authConfig,
  rawLocalConfig: {},
  registries,
  rootProjectManifestDir: '',
  registry: registries.default,
  sort: true,
  storeDir: '../store',
  strictSsl: true,
  userAgent: 'pnpm',
  userConfig: {},
  useRunningStoreServer: false,
  useStoreServer: false,
  workspaceConcurrency: 4,
  virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
  peersSuffixMaxLength: 1000,
}

export const AUDIT_REGISTRY = 'http://audit.registry/'
export const AUDIT_REGISTRY_OPTS = {
  ...DEFAULT_OPTS,
  registry: AUDIT_REGISTRY,
  registries: {
    default: AUDIT_REGISTRY,
  },
  authConfig: {
    registry: AUDIT_REGISTRY,
  },
}

export const MOCK_REGISTRY = `http://localhost:${REGISTRY_MOCK_PORT}`
export const MOCK_REGISTRY_OPTS = {
  ...DEFAULT_OPTS,
  registry: MOCK_REGISTRY,
  registries: {
    default: MOCK_REGISTRY,
  },
  authConfig: {
    registry: MOCK_REGISTRY,
  },
}
