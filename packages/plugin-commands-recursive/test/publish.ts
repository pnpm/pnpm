import { recursive } from '@pnpm/plugin-commands-recursive'
import { preparePackages } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import execa = require('execa')
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
    // This will not be published because is-positive@1.0.0 is in the registry
    {
      name: 'is-positive',
      version: '1.0.0',

      scripts: {
        prepublishOnly: 'exit 1',
      },
    },
  ])

  await fs.writeFile('.npmrc', CREDENTIALS, 'utf8')

  await recursive.handler(['publish'], {
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  })

  {
    const { stdout } = await execa('npm', ['view', '@pnpmtest/test-recursive-publish-project-1', 'versions', '--registry', `http://localhost:${REGISTRY_MOCK_PORT}`, '--json'])
    t.deepEqual(JSON.parse(stdout.toString()), ['1.0.0'])
  }
  {
    const { stdout } = await execa('npm', ['view', '@pnpmtest/test-recursive-publish-project-2', 'versions', '--registry', `http://localhost:${REGISTRY_MOCK_PORT}`, '--json'])
    t.deepEqual(JSON.parse(stdout.toString()), ['1.0.0'])
  }

  t.end()
})
