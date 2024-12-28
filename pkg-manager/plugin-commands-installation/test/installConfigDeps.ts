import { install } from '@pnpm/plugin-commands-installation'
import { prepare } from '@pnpm/prepare'
import { getIntegrity } from '@pnpm/registry-mock'
import { sync as loadJsonFile } from 'load-json-file'
import { DEFAULT_OPTS } from './utils'

test('configuration dependency is installed', async () => {
  const rootProjectManifest = {
    pnpm: {
      configDependencies: {
        '@pnpm.e2e/foo': `100.0.0+${getIntegrity('@pnpm.e2e/foo', '100.0.0')}`,
      },
    },
  }
  prepare(rootProjectManifest)

  await install.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    rootProjectManifest,
    rootProjectManifestDir: process.cwd(),
  })

  const configDepManifest = loadJsonFile<{ name: string }>('node_modules/.pnpm-config/@pnpm.e2e/foo/package.json')
  expect(configDepManifest.name).toBe('@pnpm.e2e/foo')
})
