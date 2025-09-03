import path from 'path'
import { prepareEmpty } from '@pnpm/prepare'
import { getIntegrity, REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { resolveConfigDeps } from '@pnpm/config.deps-installer'
import { createTempStore } from '@pnpm/testing.temp-store'
import { sync as readYamlFile } from 'read-yaml-file'

const registry = `http://localhost:${REGISTRY_MOCK_PORT}/`

test('configuration dependency is resolved', async () => {
  prepareEmpty()
  const { storeController } = createTempStore()

  await resolveConfigDeps(['@pnpm.e2e/foo@100.0.0'], {
    registries: {
      default: registry,
    },
    rootDir: process.cwd(),
    cacheDir: path.resolve('cache'),
    userConfig: {},
    store: storeController,
  })

  const workspaceManifest = readYamlFile<{ configDependencies: Record<string, string> }>('pnpm-workspace.yaml')
  expect(workspaceManifest.configDependencies).toStrictEqual({
    '@pnpm.e2e/foo': `100.0.0+${getIntegrity('@pnpm.e2e/foo', '100.0.0')}`,
  })
})
