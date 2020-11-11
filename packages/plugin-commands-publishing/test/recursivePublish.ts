import { readProjects } from '@pnpm/filter-workspace-packages'
import { publish } from '@pnpm/plugin-commands-publishing'
import { preparePackages } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { DEFAULT_OPTS } from './utils'
import crossSpawn = require('cross-spawn')
import execa = require('execa')
import fs = require('mz/fs')

const CREDENTIALS = [
  `--registry=http://localhost:${REGISTRY_MOCK_PORT}/`,
  `--//localhost:${REGISTRY_MOCK_PORT}/:username=username`,
  `--//localhost:${REGISTRY_MOCK_PORT}/:_password=${Buffer.from('password').toString('base64')}`,
  `--//localhost:${REGISTRY_MOCK_PORT}/:email=foo@bar.net`,
].join('\n')

jest.setTimeout(60000)

test('recursive publish', async () => {
  const pkg1 = {
    name: '@pnpmtest/test-recursive-publish-project-1',
    version: '1.0.0',

    dependencies: {
      'is-positive': '1.0.0',
    },
  }
  const pkg2 = {
    name: '@pnpmtest/test-recursive-publish-project-2',
    version: '1.0.0',

    dependencies: {
      'is-negative': '1.0.0',
    },
  }
  const projects = preparePackages(undefined, [
    pkg1,
    pkg2,
    // This will not be published because is-positive@1.0.0 is in the registry
    {
      name: 'is-positive',
      version: '1.0.0',

      scripts: {
        prepublishOnly: 'exit 1',
      },
    },
    // This will not be published because it is a private package
    {
      name: 'i-am-private',
      version: '1.0.0',

      private: true,
      scripts: {
        prepublishOnly: 'exit 1',
      },
    },
    // Package with no name is skipped
    {
      location: 'no-name',
      package: {
        scripts: {
          prepublishOnly: 'exit 1',
        },
      },
    },
  ])

  await fs.writeFile('.npmrc', CREDENTIALS, 'utf8')

  await publish.handler({
    ...DEFAULT_OPTS,
    ...await readProjects(process.cwd(), []),
    dir: process.cwd(),
    dryRun: true,
    recursive: true,
  }, [])

  {
    const { status } = crossSpawn.sync('npm', ['view', pkg1.name, 'versions', '--registry', `http://localhost:${REGISTRY_MOCK_PORT}`, '--json'])
    expect(status).toBe(1)
  }
  {
    const { status } = crossSpawn.sync('npm', ['view', pkg2.name, 'versions', '--registry', `http://localhost:${REGISTRY_MOCK_PORT}`, '--json'])
    expect(status).toBe(1)
  }

  await publish.handler({
    ...DEFAULT_OPTS,
    ...await readProjects(process.cwd(), []),
    dir: process.cwd(),
    recursive: true,
  }, [])

  {
    const { stdout } = await execa('npm', ['view', pkg1.name, 'versions', '--registry', `http://localhost:${REGISTRY_MOCK_PORT}`, '--json'])
    expect(JSON.parse(stdout.toString())).toStrictEqual([pkg1.version])
  }
  {
    const { stdout } = await execa('npm', ['view', pkg2.name, 'versions', '--registry', `http://localhost:${REGISTRY_MOCK_PORT}`, '--json'])
    expect(JSON.parse(stdout.toString())).toStrictEqual([pkg2.version])
  }

  await projects[pkg1.name].writePackageJson({ ...pkg1, version: '2.0.0' })

  await publish.handler({
    ...DEFAULT_OPTS,
    ...await readProjects(process.cwd(), []),
    dir: process.cwd(),
    recursive: true,
    tag: 'next',
  }, [])

  {
    const { stdout } = await execa('npm', ['dist-tag', 'ls', pkg1.name, '--registry', `http://localhost:${REGISTRY_MOCK_PORT}`])
    expect(stdout.toString().includes('next: 2.0.0')).toBeTruthy()
  }
})
