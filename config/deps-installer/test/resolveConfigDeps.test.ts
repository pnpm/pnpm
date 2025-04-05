import path from 'path'
import { prepareEmpty } from '@pnpm/prepare'
import { getIntegrity, REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { resolveConfigDeps } from '@pnpm/config.deps-installer'
import { sync as readYamlFile } from 'read-yaml-file'

const registry = `http://localhost:${REGISTRY_MOCK_PORT}/`

test('configuration dependency is resolved', async () => {
  prepareEmpty()

  await resolveConfigDeps(['@pnpm.e2e/foo@100.0.0'], {
    registries: {
      default: registry,
    },
    dir: process.cwd(),
    cacheDir: path.resolve('cache'),
    userConfig: {},
    rootProjectManifestDir: process.cwd(),
  })

  const workspaceManifest = readYamlFile<{ configDependencies: Record<string, string> }>('pnpm-workspace.yaml')
  expect(workspaceManifest.configDependencies).toStrictEqual({
    '@pnpm.e2e/foo': `100.0.0+${getIntegrity('@pnpm.e2e/foo', '100.0.0')}`,
  })
})
