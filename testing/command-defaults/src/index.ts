import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'

export const REGISTRY_URL = `http://localhost:${REGISTRY_MOCK_PORT}`

/**
 * Common default options shared across all command handler tests.
 * Each test suite should spread this and override only what it needs.
 */
export const DEFAULT_OPTS = {
  argv: {
    original: [],
  },
  bail: true,
  bin: 'node_modules/.bin',
  ca: undefined,
  cacheDir: '../cache',
  cert: undefined,
  cliOptions: {},
  configByUri: {},
  excludeLinksFromLockfile: false,
  extraEnv: {},
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
  minimumReleaseAgeIgnoreMissingTime: true,
  networkConcurrency: 16,
  offline: false,
  pending: false,
  pnpmfile: ['./.pnpmfile.cjs'],
  pnpmHomeDir: '',
  preferWorkspacePackages: true,
  proxy: undefined,
  registries: { default: REGISTRY_URL },
  registry: REGISTRY_URL,
  rootProjectManifestDir: '',
  sort: true,
  storeDir: '../store',
  strictSsl: false,
  userAgent: 'pnpm',
  userConfig: {},
  useRunningStoreServer: false,
  useStoreServer: false,
  workspaceConcurrency: 4,
  virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
}
