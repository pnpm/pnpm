import path from 'path'
import { add, install } from '@pnpm/plugin-commands-installation'
import { prepare } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import tempy from 'tempy'

const REGISTRY_URL = `http://localhost:${REGISTRY_MOCK_PORT}`
const TMP = tempy.directory()

const DEFAULT_OPTIONS = {
  argv: {
    original: [],
  },
  bail: false,
  bin: 'node_modules/.bin',
  cacheDir: path.join(TMP, 'cache'),
  excludeLinksFromLockfile: false,
  extraEnv: {},
  cliOptions: {},
  deployAllFiles: false,
  include: {
    dependencies: true,
    devDependencies: true,
    optionalDependencies: true,
  },
  lock: true,
  pnpmfile: '.pnpmfile.cjs',
  pnpmHomeDir: '',
  preferWorkspacePackages: true,
  rawConfig: { registry: REGISTRY_URL },
  rawLocalConfig: { registry: REGISTRY_URL },
  registries: {
    default: REGISTRY_URL,
  },
  rootProjectManifestDir: '',
  sort: true,
  storeDir: path.join(TMP, 'store'),
  userConfig: {},
  workspaceConcurrency: 1,
  virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
}

test('root dependency that has a peer is correctly updated after its version changes', async () => {
  const project = prepare({})

  await add.handler({
    ...DEFAULT_OPTIONS,
    dir: process.cwd(),
    linkWorkspacePackages: true,
  }, ['ajv@4.10.4', 'ajv-keywords@1.5.0'])

  {
    const lockfile = project.readLockfile()
    expect(lockfile.importers['.'].dependencies?.['ajv-keywords'].version).toBe('1.5.0(ajv@4.10.4)')
  }

  project.writePackageJson({
    dependencies: {
      ajv: '4.10.4',
      'ajv-keywords': '1.5.1',
    },
  })

  await install.handler({
    ...DEFAULT_OPTIONS,
    dir: process.cwd(),
    linkWorkspacePackages: true,
    rawLocalConfig: {
      'frozen-lockfile': false,
    },
  })

  {
    const lockfile = project.readLockfile()
    expect(lockfile.importers['.'].dependencies?.['ajv-keywords'].version).toBe('1.5.1(ajv@4.10.4)')
  }
})
