import { promises as fs } from 'fs'
import path from 'path'
import { add } from '@pnpm/plugin-commands-installation'
import prepare from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import tempy from 'tempy'
import nodeExecPath from '../lib/nodeExecPath'

const REGISTRY_URL = `http://localhost:${REGISTRY_MOCK_PORT}`
const tmp = tempy.directory()

const DEFAULT_OPTIONS = {
  argv: {
    original: [],
  },
  bail: false,
  bin: 'node_modules/.bin',
  cacheDir: path.join(tmp, 'cache'),
  cliOptions: {},
  include: {
    dependencies: true,
    devDependencies: true,
    optionalDependencies: true,
  },
  lock: true,
  pnpmfile: '.pnpmfile.cjs',
  rawConfig: { registry: REGISTRY_URL },
  rawLocalConfig: { registry: REGISTRY_URL },
  registries: {
    default: REGISTRY_URL,
  },
  sort: true,
  storeDir: path.join(tmp, 'store'),
  workspaceConcurrency: 1,
}

test('globally installed package is linked with active version of Node.js', async () => {
  prepare()
  await add.handler({
    ...DEFAULT_OPTIONS,
    dir: process.cwd(),
    global: true,
    linkWorkspacePackages: false,
  }, ['hello-world-js-bin'])

  const manifest = (await import(path.resolve('package.json')))

  expect(
    manifest.dependenciesMeta['hello-world-js-bin']?.node
  ).toBeTruthy()

  const shimContent = await fs.readFile('node_modules/.bin/hello-world-js-bin', 'utf-8')
  expect(shimContent).toContain(await nodeExecPath())
})
