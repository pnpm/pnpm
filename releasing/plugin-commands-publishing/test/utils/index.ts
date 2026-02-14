import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import execa from 'execa'

const REGISTRY = `http://localhost:${REGISTRY_MOCK_PORT}`

export const DEFAULT_OPTS = {
  authInfos: {},
  argv: {
    original: [],
  },
  bail: false,
  bin: 'node_modules/.bin',
  ca: undefined,
  cert: undefined,
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
  networkConcurrency: 16,
  offline: false,
  pending: false,
  pnpmfile: ['./.pnpmfile.cjs'],
  pnpmHomeDir: '',
  proxy: undefined,
  rawConfig: { registry: REGISTRY },
  rawLocalConfig: {},
  registries: { default: REGISTRY },
  registry: REGISTRY,
  sort: true,
  cacheDir: '../cache',
  strictSsl: false,
  sslConfigs: {},
  userAgent: 'pnpm',
  userConfig: {},
  useRunningStoreServer: false,
  useStoreServer: false,
  workspaceConcurrency: 4,
  virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
}

export async function checkPkgExists (packageName: string, expectedVersion: string): Promise<void> {
  const { stdout } = await execa('npm', ['view', packageName, 'versions', '--registry', `http://localhost:${REGISTRY_MOCK_PORT}`, '--json'])
  const output = JSON.parse(stdout.toString())
  expect(Array.isArray(output) ? output[0] : output).toStrictEqual(expectedVersion)
}
