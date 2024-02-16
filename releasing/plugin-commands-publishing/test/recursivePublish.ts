import { promises as fs } from 'fs'
import path from 'path'
import { readProjects } from '@pnpm/filter-workspace-packages'
import { streamParser } from '@pnpm/logger'
import { publish } from '@pnpm/plugin-commands-publishing'
import { preparePackages } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { type ProjectManifest } from '@pnpm/types'
import execa from 'execa'
import crossSpawn from 'cross-spawn'
import loadJsonFile from 'load-json-file'
import { DEFAULT_OPTS } from './utils'

const CREDENTIALS = `\
registry=http://localhost:${REGISTRY_MOCK_PORT}/
//localhost:${REGISTRY_MOCK_PORT}/:username=username
//localhost:${REGISTRY_MOCK_PORT}/:_password=${Buffer.from('password').toString('base64')}
//localhost:${REGISTRY_MOCK_PORT}/:email=foo@bar.net`

test('recursive publish', async () => {
  // This suffix is added to the package name to avoid issue if Jest reruns the test
  const SUFFIX = Date.now()

  const pkg1 = {
    name: `@pnpmtest/test-recursive-publish-project-1-${SUFFIX}`,
    version: '1.0.0',

    dependencies: {
      'is-positive': '1.0.0',
    },
  }
  const pkg2 = {
    name: `@pnpmtest/test-recursive-publish-project-2-${SUFFIX}`,
    version: '1.0.0',

    dependencies: {
      'is-negative': '1.0.0',
    },
  }
  const projects = preparePackages([
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

  process.env.npm_config_userconfig = path.join('.npmrc')
  await publish.handler({
    ...DEFAULT_OPTS,
    ...await readProjects(process.cwd(), []),
    dir: process.cwd(),
    recursive: true,
  }, [])

  {
    const { stdout } = await execa('npm', ['view', pkg1.name, 'versions', '--registry', `http://localhost:${REGISTRY_MOCK_PORT}`, '--json'])
    expect(JSON.parse(stdout.toString())).toStrictEqual(pkg1.version)
  }
  {
    const { stdout } = await execa('npm', ['view', pkg2.name, 'versions', '--registry', `http://localhost:${REGISTRY_MOCK_PORT}`, '--json'])
    expect(JSON.parse(stdout.toString())).toStrictEqual(pkg2.version)
  }

  projects[pkg1.name].writePackageJson({ ...pkg1, version: '2.0.0' })

  await publish.handler({
    ...DEFAULT_OPTS,
    ...await readProjects(process.cwd(), []),
    dir: process.cwd(),
    recursive: true,
    tag: 'next',
  }, [])

  {
    const { stdout } = await execa('npm', ['dist-tag', 'ls', pkg1.name, '--registry', `http://localhost:${REGISTRY_MOCK_PORT}`])
    expect(stdout.toString()).toContain('next: 2.0.0')
  }
})

test('print info when no packages are published', async () => {
  preparePackages([
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

  const reporter = jest.fn()
  streamParser.on('data', reporter)

  await publish.handler({
    ...DEFAULT_OPTS,
    ...await readProjects(process.cwd(), []),
    dir: process.cwd(),
    dryRun: true,
    recursive: true,
  }, [])

  streamParser.removeListener('data', reporter)
  expect(reporter).toBeCalledWith(expect.objectContaining({
    level: 'info',
    message: 'There are no new packages that should be published',
    name: 'pnpm',
    prefix: process.cwd(),
  }))
})

test('packages are released even if their current version is published, when force=true', async () => {
  preparePackages([
    // This version is already in the registry
    {
      name: 'is-positive',
      version: '3.1.0',

      scripts: {
        prepublishOnly: 'pnpm version major',
      },
    },
  ])

  await fs.writeFile('.npmrc', CREDENTIALS, 'utf8')

  await publish.handler({
    ...DEFAULT_OPTS,
    ...await readProjects(process.cwd(), []),
    force: true,
    dir: process.cwd(),
    dryRun: true,
    recursive: true,
  }, [])

  const manifest = await loadJsonFile<ProjectManifest>('is-positive/package.json')
  expect(manifest.version).toBe('4.0.0')
})

test('recursive publish writes publish summary', async () => {
  preparePackages([
    {
      name: '@pnpmtest/test-recursive-publish-project-3',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
      },
    },
    {
      name: '@pnpmtest/test-recursive-publish-project-4',
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

  process.env.npm_config_userconfig = path.join('.npmrc')
  await publish.handler({
    ...DEFAULT_OPTS,
    ...await readProjects(process.cwd(), []),
    dir: process.cwd(),
    recursive: true,
    reportSummary: true,
  }, [])

  {
    const publishSummary = await loadJsonFile('pnpm-publish-summary.json')
    expect(publishSummary).toMatchSnapshot()
    await fs.unlink('pnpm-publish-summary.json')
  }

  await publish.handler({
    ...DEFAULT_OPTS,
    ...await readProjects(process.cwd(), []),
    dir: process.cwd(),
    recursive: true,
    reportSummary: true,
  }, [])

  {
    const publishSummary = await loadJsonFile('pnpm-publish-summary.json')
    expect(publishSummary).toStrictEqual({
      publishedPackages: [],
    })
  }
})

test('when publish some package throws an error, exit code should be non-zero', async () => {
  preparePackages([
    {
      name: '@pnpmtest/test-recursive-publish-project-5',
      version: '1.0.0',
    },
    {
      name: '@pnpmtest/test-recursive-publish-project-6',
      version: '1.0.0',
    },
  ])

  // Throw ENEEDAUTH error when publish.
  await fs.writeFile('.npmrc', 'registry=https://__fake_npm_registry__.com', 'utf8')

  const result = await publish.handler({
    ...DEFAULT_OPTS,
    ...await readProjects(process.cwd(), []),
    dir: process.cwd(),
    recursive: true,
    force: true,
  }, [])

  expect(result?.exitCode).toBe(1)
})
