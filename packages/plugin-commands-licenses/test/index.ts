/// <reference path="../../../typings/index.d.ts" />
import path from 'path'
import { licenses } from '@pnpm/plugin-commands-licenses'
import { install } from '@pnpm/plugin-commands-installation'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import stripAnsi from 'strip-ansi'
import { DEFAULT_OPTS } from './utils'
import tempy from 'tempy'

const REGISTRY_URL = `http://localhost:${REGISTRY_MOCK_PORT}`

const LICENSES_OPTIONS = {
  cacheDir: 'cache',
  fetchRetries: 1,
  fetchRetryFactor: 1,
  fetchRetryMaxtimeout: 60,
  fetchRetryMintimeout: 10,
  global: false,
  networkConcurrency: 16,
  offline: false,
  rawConfig: { registry: REGISTRY_URL },
  registries: { default: REGISTRY_URL },
  strictSsl: false,
  tag: 'latest',
  userAgent: '',
  userConfig: {},
}

test('pnpm licenses', async () => {
  const workspaceDir = path.resolve('./test/fixtures/complex-licenses')

  const tmp = tempy.directory()
  const storeDir = path.join(tmp, 'store')
  await install.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    pnpmHomeDir: '',
    storeDir,
  })

  // Attempt to run the licenses command now
  const { output, exitCode } = await licenses.handler({
    ...LICENSES_OPTIONS,
    dir: workspaceDir,
    pnpmHomeDir: '',
    long: false,
    // we need to prefix it with v3 otherwise licenses tool can't find anything
    // in the content-addressable directory
    storeDir: path.resolve(storeDir, 'v3'),
  }, ['list'])

  expect(exitCode).toBe(0)
  expect(stripAnsi(output)).toMatchSnapshot('show-packages')
})

test('pnpm licenses: show details', async () => {
  const workspaceDir = path.resolve('./test/fixtures/simple-licenses')

  const tmp = tempy.directory()
  const storeDir = path.join(tmp, 'store')
  await install.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    pnpmHomeDir: '',
    storeDir,
  })

  // Attempt to run the licenses command now
  const { output, exitCode } = await licenses.handler({
    ...LICENSES_OPTIONS,
    dir: workspaceDir,
    pnpmHomeDir: '',
    long: true,
    // we need to prefix it with v3 otherwise licenses tool can't find anything
    // in the content-addressable directory
    storeDir: path.resolve(storeDir, 'v3'),
  }, ['list'])

  expect(exitCode).toBe(0)
  expect(stripAnsi(output)).toMatchSnapshot('show-packages-details')
})

test('pnpm licenses: output as json', async () => {
  const workspaceDir = path.resolve('./test/fixtures/simple-licenses')

  const tmp = tempy.directory()
  const storeDir = path.join(tmp, 'store')
  await install.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    pnpmHomeDir: '',
    storeDir,
  })

  // Attempt to run the licenses command now
  const { output, exitCode } = await licenses.handler({
    ...LICENSES_OPTIONS,
    dir: workspaceDir,
    pnpmHomeDir: '',
    long: false,
    json: true,
    // we need to prefix it with v3 otherwise licenses tool can't find anything
    // in the content-addressable directory
    storeDir: path.resolve(storeDir, 'v3'),
  }, ['list'])

  expect(exitCode).toBe(0)
  expect(output).not.toHaveLength(0)
  expect(output).not.toBe('No licenses in packages found')
  const parsedOutput = JSON.parse(output)
  expect(Object.keys(parsedOutput)).toMatchSnapshot('found-license-types')
  const packagesWithMIT = parsedOutput['MIT']
  expect(packagesWithMIT.length).toBeGreaterThan(0)
  expect(Object.keys(packagesWithMIT[0])).toEqual([
    'name',
    'version',
    'path',
    'license',
    'author',
    'homepage',
  ])
  expect(packagesWithMIT[0].name).toBe('is-positive')
})

test('pnpm licenses: fails when lockfile is missing', async () => {
  await expect(
    licenses.handler({
      ...LICENSES_OPTIONS,
      dir: path.resolve('./test/fixtures/invalid'),
      pnpmHomeDir: '',
      long: true,
    }, ['list'])
  ).rejects.toThrowErrorMatchingInlineSnapshot(
    '"No pnpm-lock.yaml found: Cannot check a project without a lockfile"'
  )
})
