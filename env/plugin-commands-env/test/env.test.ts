import { PnpmError } from '@pnpm/error'
import { env } from '@pnpm/plugin-commands-env'
import { tempDir } from '@pnpm/prepare'
import * as execa from 'execa'
import fs from 'fs'
import nock from 'nock'
import path from 'path'
import PATH from 'path-name'
import { temporaryDirectory } from 'tempy'

test('install Node (and npm, npx) by exact version of Node.js', async () => {
  tempDir()
  const configDir = path.resolve('config')

  await env.handler({
    bin: process.cwd(),
    cacheDir: temporaryDirectory(),
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

  // Node.js is now installed in the synthetic env project's node_modules
  expect(fs.existsSync(path.resolve('env', 'node_modules', 'node'))).toBeTruthy()

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

test('fail if there is no global bin directory', async () => {
  tempDir()

  await expect(
    env.handler({
      // @ts-expect-error
      bin: undefined,
      global: true,
      pnpmHomeDir: process.cwd(),
      rawConfig: {},
    }, ['use', 'lts'])
  ).rejects.toEqual(new PnpmError('CANNOT_MANAGE_NODE', 'Unable to manage Node.js because pnpm was not installed using the standalone installation script'))
})

test('use overrides the previous Node.js version', async () => {
  tempDir()
  const configDir = path.resolve('config')
  const cacheDir = temporaryDirectory()

  await env.handler({
    bin: process.cwd(),
    cacheDir,
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

  await env.handler({
    bin: process.cwd(),
    cacheDir,
    configDir,
    global: true,
    pnpmHomeDir: process.cwd(),
    rawConfig: {},
  }, ['use', '16.5.0'])

  {
    const { stdout } = execa.sync('node', ['-v'], opts)
    expect(stdout.toString()).toBe('v16.5.0')
  }
})
