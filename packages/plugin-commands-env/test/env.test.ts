import fs from 'fs'
import path from 'path'
import PnpmError from '@pnpm/error'
import { tempDir } from '@pnpm/prepare'
import { env } from '@pnpm/plugin-commands-env'
import execa from 'execa'
import PATH from 'path-name'

test('install Node (and npm, npx) by exact version of Node.js', async () => {
  tempDir()

  await env.handler({
    bin: process.cwd(),
    global: true,
    pnpmHomeDir: process.cwd(),
    rawConfig: {},
  }, ['use', '16.4.0'])

  const opts = {
    env: {
      [PATH]: `${process.cwd()}${path.delimiter}${process.env[PATH] as string}`,
    },
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
})

test('install Node by version range', async () => {
  tempDir()

  await env.handler({
    bin: process.cwd(),
    global: true,
    pnpmHomeDir: process.cwd(),
    rawConfig: {},
  }, ['use', '6'])

  const { stdout } = execa.sync('node', ['-v'], {
    env: {
      [PATH]: `${process.cwd()}${path.delimiter}${process.env[PATH] as string}`,
    },
  })
  expect(stdout.toString()).toBe('v6.17.1')

  const dirs = fs.readdirSync(path.resolve('nodejs'))
  expect(dirs).toEqual(['6.17.1'])
})

test('install the LTS version of Node', async () => {
  tempDir()

  await env.handler({
    bin: process.cwd(),
    global: true,
    pnpmHomeDir: process.cwd(),
    rawConfig: {},
  }, ['use', 'lts'])

  const { stdout: version } = execa.sync('node', ['-v'], {
    env: {
      [PATH]: `${process.cwd()}${path.delimiter}${process.env[PATH] as string}`,
    },
  })
  expect(version).toBeTruthy()

  const dirs = fs.readdirSync(path.resolve('nodejs'))
  expect(dirs).toEqual([version.substring(1)])
})

test('install Node by its LTS name', async () => {
  tempDir()

  await env.handler({
    bin: process.cwd(),
    global: true,
    pnpmHomeDir: process.cwd(),
    rawConfig: {},
  }, ['use', 'argon'])

  const { stdout: version } = execa.sync('node', ['-v'], {
    env: {
      [PATH]: `${process.cwd()}${path.delimiter}${process.env[PATH] as string}`,
    },
  })
  expect(version).toBe('v4.9.1')

  const dirs = fs.readdirSync(path.resolve('nodejs'))
  expect(dirs).toEqual([version.substring(1)])
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
