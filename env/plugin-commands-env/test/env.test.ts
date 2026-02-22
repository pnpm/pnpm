import { env } from '@pnpm/plugin-commands-env'
import { tempDir } from '@pnpm/prepare'
import * as execa from 'execa'
import path from 'path'
import PATH from 'path-name'
import { temporaryDirectory } from 'tempy'

test('install Node (and npm, npx) by exact version of Node.js', async () => {
  tempDir()

  await env.handler({
    bin: process.cwd(),
    cacheDir: temporaryDirectory(),
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
  ).rejects.toThrow()
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
  ).rejects.toThrow()
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
  ).rejects.toThrow('Unable to manage Node.js because pnpm was not installed using the standalone installation script')
})

test('use overrides the previous Node.js version', async () => {
  tempDir()
  const cacheDir = temporaryDirectory()

  await env.handler({
    bin: process.cwd(),
    cacheDir,
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
    global: true,
    pnpmHomeDir: process.cwd(),
    rawConfig: {},
  }, ['use', '16.5.0'])

  {
    const { stdout } = execa.sync('node', ['-v'], opts)
    expect(stdout.toString()).toBe('v16.5.0')
  }
})
