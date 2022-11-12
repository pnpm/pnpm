import path from 'path'
import { PnpmError } from '@pnpm/error'
import { tempDir } from '@pnpm/prepare'
import { env } from '@pnpm/plugin-commands-env'
import semver from 'semver'
import { listLocalVersions, listRemoteVersions } from '../lib/envList'

test('list local versions', async () => {
  tempDir()
  const configDir = path.resolve('config')

  await env.handler({
    bin: process.cwd(),
    configDir,
    global: true,
    pnpmHomeDir: process.cwd(),
    rawConfig: {},
  }, ['use', '16.4.0'])

  const { currentVersion, versions } = await listLocalVersions({
    bin: process.cwd(),
    configDir,
    pnpmHomeDir: process.cwd(),
    rawConfig: {},
  })

  expect(currentVersion).toEqual('16.4.0')
  expect(versions).toEqual(['16.4.0'])
})

test('list local versions failed if Node.js directory not found', async () => {
  tempDir()
  const configDir = path.resolve('config')
  const pnpmHomeDir = path.join(process.cwd(), 'specified-dir')

  await expect(
    listLocalVersions({
      bin: process.cwd(),
      configDir,
      pnpmHomeDir,
      rawConfig: {},
    })
  ).rejects.toEqual(new PnpmError('ENV_NO_NODE_DIRECTORY', `Couldn't find Node.js directory in ${path.join(pnpmHomeDir, 'nodejs')}`))
})

test('list remote versions', async () => {
  tempDir()
  const configDir = path.resolve('config')

  const versions = await listRemoteVersions({
    bin: process.cwd(),
    configDir,
    pnpmHomeDir: process.cwd(),
    rawConfig: {},
  }, '16')
  expect(versions.every(version => semver.satisfies(version, '16'))).toBeTruthy()
})
