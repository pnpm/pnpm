import fs from 'fs'
import path from 'path'
import { PnpmError } from '@pnpm/error'
import { tempDir } from '@pnpm/prepare'
import { env, node } from '@pnpm/plugin-commands-env'
import * as execa from 'execa'
import nock from 'nock'
import PATH from 'path-name'
import semver from 'semver'

test('install Node (and npm, npx) by exact version of Node.js', async () => {
  tempDir()
  const configDir = path.resolve('config')

  await env.handler({
    bin: process.cwd(),
    configDir,
    global: true,
    pnpmHomeDir: process.cwd(),
    rawConfig: {},
  }, ['use', '16.4.0'])

  const opts = {
    env: {
      [PATH]: `${process.cwd()}${path.delimiter}${process.env[PATH] as string}`,
    },
    extendEnv: false,
  }

  {
    const { stdout } = execa.sync('node', ['-v'], opts)
    expect(stdout.toString()).toBe('v16.4.0')
  }

  {
    const { stdout } = execa.sync('npm', ['-v'], opts)
    expect(stdout.toString()).toBe('7.18.1')
  }

  {
    const { stdout } = execa.sync('npx', ['-v'], opts)
    expect(stdout.toString()).toBe('7.18.1')
  }

  const dirs = fs.readdirSync(path.resolve('nodejs'))
  expect(dirs).toEqual(['16.4.0'])

  {
    const { stdout } = execa.sync('npm', ['config', 'get', 'globalconfig'], opts)
    expect(stdout.toString()).toBe(path.join(configDir, 'npmrc'))
  }
})

test('resolveNodeVersion uses node-mirror:release option', async () => {
  tempDir()
  const configDir = path.resolve('config')

  const nockScope = nock('https://pnpm-node-mirror-test.localhost')
    .get('/download/release/index.json')
    .reply(200, [])

  await expect(
    env.handler({
      bin: process.cwd(),
      configDir,
      global: true,
      pnpmHomeDir: process.cwd(),
      rawConfig: {
        'node-mirror:release': 'https://pnpm-node-mirror-test.localhost/download/release',
      },
    }, ['use', '16.4.0'])
  ).rejects.toEqual(new PnpmError('COULD_NOT_RESOLVE_NODEJS', 'Couldn\'t find Node.js version matching 16.4.0'))

  expect(nockScope.isDone()).toBeTruthy()
})

test('fail if a non-existed Node.js version is tried to be installed', async () => {
  tempDir()

  await expect(
    env.handler({
      bin: process.cwd(),
      global: true,
      pnpmHomeDir: process.cwd(),
      rawConfig: {},
    }, ['use', '6.999'])
  ).rejects.toEqual(new PnpmError('COULD_NOT_RESOLVE_NODEJS', 'Couldn\'t find Node.js version matching 6.999'))
})

test('fail if a non-existed Node.js LTS is tried to be installed', async () => {
  tempDir()

  await expect(
    env.handler({
      bin: process.cwd(),
      global: true,
      pnpmHomeDir: process.cwd(),
      rawConfig: {},
    }, ['use', 'boo'])
  ).rejects.toEqual(new PnpmError('COULD_NOT_RESOLVE_NODEJS', 'Couldn\'t find Node.js version matching boo'))
})

// Regression test for https://github.com/pnpm/pnpm/issues/4104
test('it re-attempts failed downloads', async () => {
  tempDir()

  // This fixture was retrieved from http://nodejs.org/download/release/index.json on 2021-12-12.
  const testReleaseInfoPath = path.join(__dirname, './fixtures/node-16.4.0-release-info.json')

  const nockScope = nock('https://nodejs.org')
    // Using nock's persist option since the default fetcher retries requests.
    .persist()
    .get('/download/release/index.json')
    .replyWithFile(200, testReleaseInfoPath)
    .persist()
    .get(uri => uri.startsWith('/download/release/v16.4.0/'))
    .replyWithError('Intentionally failing response for test')

  try {
    const attempts = 2
    for (let i = 0; i < attempts; i++) {
      await expect(
        env.handler({
          bin: process.cwd(),
          global: true,
          pnpmHomeDir: process.cwd(),
          rawConfig: {},
        }, ['use', '16.4.0'])
      ).rejects.toThrow('Intentionally failing response for test')
    }

    expect(nockScope.isDone()).toBeTruthy()
  } finally {
    nock.cleanAll()
  }
})

describe('env remove', () => {
  test('fail if --global is missing', async () => {
    tempDir()

    await expect(
      env.handler({
        bin: process.cwd(),
        global: false,
        pnpmHomeDir: process.cwd(),
        rawConfig: {},
      }, ['remove', 'lts'])
    ).rejects.toEqual(new PnpmError('NOT_IMPLEMENTED_YET', '"pnpm env use <version>" can only be used with the "--global" option currently'))
  })

  test('fail if can not resolve Node.js version', async () => {
    tempDir()

    await expect(
      env.handler({
        bin: process.cwd(),
        global: true,
        pnpmHomeDir: process.cwd(),
        rawConfig: {},
      }, ['rm', 'non-existing-version'])
    ).rejects.toEqual(new PnpmError('COULD_NOT_RESOLVE_NODEJS', 'Couldn\'t find Node.js version matching non-existing-version'))
  })

  test('fail if trying to remove version that is not installed', async () => {
    tempDir()

    const nodeDir = node.getNodeVersionsBaseDir(process.cwd())

    await expect(
      env.handler({
        bin: process.cwd(),
        global: true,
        pnpmHomeDir: process.cwd(),
        rawConfig: {},
      }, ['remove', '16.4.0'])
    ).rejects.toEqual(new PnpmError('ENV_NO_NODE_DIRECTORY', `Couldn't find Node.js directory in ${path.resolve(nodeDir, '16.4.0')}`))
  })

  test('install and remove Node.js by exact version', async () => {
    tempDir()

    const configDir = path.resolve('config')

    await env.handler({
      bin: process.cwd(),
      configDir,
      global: true,
      pnpmHomeDir: process.cwd(),
      rawConfig: {},
    }, ['use', '16.4.0'])

    const opts = {
      env: {
        [PATH]: process.cwd(),
      },
      extendEnv: false,
    }

    {
      const { stdout } = execa.sync('node', ['-v'], opts)
      expect(stdout.toString()).toBe('v16.4.0')
    }

    await env.handler({
      bin: process.cwd(),
      global: true,
      pnpmHomeDir: process.cwd(),
      rawConfig: {},
    }, ['rm', '16.4.0'])

    expect(() => execa.sync('node', ['-v'], opts)).toThrowError()
  })
})

describe('env list', () => {
  test('list local Node.js versions', async () => {
    tempDir()
    const configDir = path.resolve('config')

    await env.handler({
      bin: process.cwd(),
      configDir,
      global: true,
      pnpmHomeDir: process.cwd(),
      rawConfig: {},
    }, ['use', '16.4.0'])

    const version = await env.handler({
      bin: process.cwd(),
      configDir,
      pnpmHomeDir: process.cwd(),
      rawConfig: {},
    }, ['list'])

    expect(version).toMatch('16.4.0')
  })
  test('list local versions fails if Node.js directory not found', async () => {
    tempDir()
    const configDir = path.resolve('config')
    const pnpmHomeDir = path.resolve('specified-dir')

    await expect(
      env.handler({
        bin: process.cwd(),
        configDir,
        pnpmHomeDir,
        rawConfig: {},
      }, ['list'])
    ).rejects.toEqual(new PnpmError('ENV_NO_NODE_DIRECTORY', `Couldn't find Node.js directory in ${path.join(pnpmHomeDir, 'nodejs')}`))
  })
  test('list remote Node.js versions', async () => {
    tempDir()
    const configDir = path.resolve('config')

    const versionStr = await env.handler({
      bin: process.cwd(),
      configDir,
      pnpmHomeDir: process.cwd(),
      rawConfig: {},
      remote: true,
    }, ['list', '16'])

    const versions = versionStr.split('\n')
    expect(versions.every(version => semver.satisfies(version, '16'))).toBeTruthy()
  })
})
