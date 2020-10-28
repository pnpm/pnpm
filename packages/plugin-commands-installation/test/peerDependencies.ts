import { add, install } from '@pnpm/plugin-commands-installation'
import prepare from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import tempy = require('tempy')

const REGISTRY_URL = `http://localhost:${REGISTRY_MOCK_PORT}`

const DEFAULT_OPTIONS = {
  argv: {
    original: [],
  },
  bail: false,
  cliOptions: {},
  include: {
    dependencies: true,
    devDependencies: true,
    optionalDependencies: true,
  },
  lock: true,
  pnpmfile: 'pnpmfile.js',
  rawConfig: { registry: REGISTRY_URL },
  rawLocalConfig: { registry: REGISTRY_URL },
  registries: {
    default: REGISTRY_URL,
  },
  sort: true,
  storeDir: tempy.directory(),
  workspaceConcurrency: 1,
}

test('root dependency that has a peer is correctly updated after its version changes', async () => {
  const project = prepare(undefined, {})

  await add.handler({
    ...DEFAULT_OPTIONS,
    dir: process.cwd(),
    linkWorkspacePackages: true,
  }, ['ajv@4.10.4', 'ajv-keywords@1.5.0'])

  {
    const lockfile = await project.readLockfile()
    expect(lockfile.dependencies['ajv-keywords']).toBe('1.5.0_ajv@4.10.4')
  }

  await project.writePackageJson({
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
    const lockfile = await project.readLockfile()
    expect(lockfile.dependencies['ajv-keywords']).toBe('1.5.1_ajv@4.10.4')
  }
})
