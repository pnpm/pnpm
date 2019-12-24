import { filterPkgsBySelectorObjects, PackageSelector } from '@pnpm/filter-workspace-packages'
import findWorkspacePackages from '@pnpm/find-workspace-packages'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'

const REGISTRY = `http://localhost:${REGISTRY_MOCK_PORT}`

export const DEFAULT_OPTS = {
  alwaysAuth: false,
  argv: {
    original: [],
  },
  bail: false,
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
  pnpmfile: './pnpmfile.js',
  proxy: undefined,
  rawConfig: { registry:  REGISTRY },
  rawLocalConfig: {},
  registries: { default: REGISTRY },
  registry: REGISTRY,
  sort: true,
  storeDir: '../store',
  strictSsl: false,
  userAgent: 'pnpm',
  useRunningStoreServer: false,
  useStoreServer: false,
  workspaceConcurrency: 4,
}

export async function readWsPkgs (
  workspaceDir: string,
  pkgSelectors: PackageSelector[],
) {
  const allWsPkgs = await findWorkspacePackages(workspaceDir, {})
  const selectedWsPkgsGraph = await filterPkgsBySelectorObjects(
    allWsPkgs,
    pkgSelectors,
    {
      workspaceDir,
    },
  )
  return { allWsPkgs, selectedWsPkgsGraph }
}
