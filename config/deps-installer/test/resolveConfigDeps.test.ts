import path from 'path'
import { prepareEmpty } from '@pnpm/prepare'
import { getIntegrity, REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { resolveConfigDeps, readConfigLockfile } from '@pnpm/config.deps-installer'
import { createTempStore } from '@pnpm/testing.temp-store'
import { sync as readYamlFile } from 'read-yaml-file'

const registry = `http://localhost:${REGISTRY_MOCK_PORT}/`

test('configuration dependency is resolved', async () => {
  prepareEmpty()
  const { storeController, storeDir } = createTempStore()

  await resolveConfigDeps(['@pnpm.e2e/foo@100.0.0'], {
    registries: {
      default: registry,
    },
    rootDir: process.cwd(),
    cacheDir: path.resolve('cache'),
    userConfig: {},
    store: storeController,
    storeDir,
  })

  // Workspace manifest should have a clean specifier (no integrity)
  const workspaceManifest = readYamlFile<{ configDependencies: Record<string, string> }>('pnpm-workspace.yaml')
  expect(workspaceManifest.configDependencies).toStrictEqual({
    '@pnpm.e2e/foo': '100.0.0',
  })

  // Config lockfile should contain the resolved dependency with integrity
  const configLockfile = await readConfigLockfile(process.cwd())
  expect(configLockfile).not.toBeNull()
  expect(configLockfile!.importers['.'].configDependencies['@pnpm.e2e/foo']).toStrictEqual({
    specifier: '100.0.0',
    version: '100.0.0',
  })
  expect(configLockfile!.packages['@pnpm.e2e/foo@100.0.0']).toStrictEqual({
    resolution: {
      integrity: getIntegrity('@pnpm.e2e/foo', '100.0.0'),
    },
  })
  expect(configLockfile!.snapshots['@pnpm.e2e/foo@100.0.0']).toStrictEqual({})
})
