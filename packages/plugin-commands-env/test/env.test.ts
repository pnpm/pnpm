import fs from 'fs'
import path from 'path'
import PnpmError from '@pnpm/error'
import { tempDir } from '@pnpm/prepare'
import { env } from '@pnpm/plugin-commands-env'
import * as execa from 'execa'
import nock from 'nock'
import PATH from 'path-name'

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

test('fail if a non-existend Node.js version is tried to be installed', async () => {
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

test('fail if a non-existend Node.js LTS is tried to be installed', async () => {
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
