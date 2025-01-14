import path from 'path'
import * as enquirer from 'enquirer'
import { approveBuilds } from '@pnpm/exec.build-commands'
import { install } from '@pnpm/plugin-commands-installation'
import { prepare } from '@pnpm/prepare'
import { type ProjectManifest } from '@pnpm/types'
import { getConfig } from '@pnpm/config'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { sync as loadJsonFile } from 'load-json-file'
import omit from 'ramda/src/omit'

jest.mock('enquirer', () => ({ prompt: jest.fn() }))

// eslint-disable-next-line
const prompt = enquirer.prompt as any

test('approve all builds', async () => {
  prepare({
    dependencies: {
      '@pnpm.e2e/pre-and-postinstall-scripts-example': '1.0.0',
      '@pnpm.e2e/install-script-example': '*',
    },
  })
  const cliOptions = {
    argv: [],
    dir: process.cwd(),
    registry: `http://localhost:${REGISTRY_MOCK_PORT}`,
    'store-dir': path.resolve('store'),
    'cache-dir': path.resolve('cache'),
  }
  const config = omit(['reporter'], (await getConfig({
    cliOptions,
    packageManager: { name: 'pnpm', version: '' },
  })).config)
  await install.handler({ ...config, argv: { original: [] } })

  prompt.mockResolvedValueOnce({
    result: [
      {
        value: '@pnpm.e2e/pre-and-postinstall-scripts-example',
      },
    ],
  })
  prompt.mockResolvedValueOnce({
    build: true,
  })

  await approveBuilds.handler(config)

  const manifest = loadJsonFile<ProjectManifest>(path.resolve('package.json'))
  expect(manifest.pnpm?.onlyBuiltDependencies).toStrictEqual(['@pnpm.e2e/pre-and-postinstall-scripts-example'])
  expect(manifest.pnpm?.ignoredBuiltDependencies).toStrictEqual(['@pnpm.e2e/install-script-example'])
})
