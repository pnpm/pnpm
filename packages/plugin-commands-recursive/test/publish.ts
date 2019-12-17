import { recursive } from '@pnpm/plugin-commands-recursive'
import { preparePackages } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import fs = require('mz/fs')
import test = require('tape')
import { DEFAULT_OPTS } from './utils'

const CREDENTIALS = [
  `--registry=http://localhost:${REGISTRY_MOCK_PORT}/`,
  `--//localhost:${REGISTRY_MOCK_PORT}/:username=username`,
  `--//localhost:${REGISTRY_MOCK_PORT}/:_password=${Buffer.from('password').toString('base64')}`,
  `--//localhost:${REGISTRY_MOCK_PORT}/:email=foo@bar.net`,
].join('\n')

test('recursive publish', async (t) => {
  const projects = preparePackages(t, [
    {
      name: '@pnpmtest/test-recursive-publish-project-1',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
      },
    },
    {
      name: '@pnpmtest/test-recursive-publish-project-2',
      version: '1.0.0',

      dependencies: {
        'is-negative': '1.0.0',
      },
    },
  ])

  await fs.writeFile('.npmrc', CREDENTIALS, 'utf8')

  await recursive.handler(['publish'], {
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  })

  t.end()
})
