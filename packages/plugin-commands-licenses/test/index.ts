/// <reference path="../../../typings/index.d.ts" />
import path from 'path'
import { licenses } from '@pnpm/plugin-commands-licenses'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import stripAnsi from 'strip-ansi'
import { PackageManifest } from '@pnpm/types'

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

jest.mock('@pnpm/read-package-json', () => ({
  readPackageJson: async (pkgPath: string) => {
    // mock the readPackageJson-call used in getPkgInfo to ensure
    // it returns a PackageManifest as in the tests we don't actually
    // have a content store or node_modules directory to fetch the
    // package.json files from
    return {
      license: 'MIT',
      homepage: 'https://pnpm.io',
      author: 'Jane Doe',
    } as PackageManifest
  },
}))

test('pnpm licenses', async () => {
  const { output, exitCode } = await licenses.handler({
    ...LICENSES_OPTIONS,
    dir: path.resolve('./test/fixtures/has-licenses'),
    long: false,
  })

  expect(exitCode).toBe(0)
  expect(stripAnsi(output)).toMatchSnapshot('show-packages')
})

test('pnpm licenses: show details', async () => {
  const { output, exitCode } = await licenses.handler({
    ...LICENSES_OPTIONS,
    dir: path.resolve('./test/fixtures/has-licenses'),
    long: true,
  })

  expect(exitCode).toBe(0)
  expect(stripAnsi(output)).toMatchSnapshot('show-packages-details')
})

test('pnpm licenses: output as json', async () => {
  const workspaceDir = path.resolve('./test/fixtures/has-licenses')
  const { output, exitCode } = await licenses.handler({
    ...LICENSES_OPTIONS,
    dir: workspaceDir,
    long: false,
    json: true,
  })

  expect(exitCode).toBe(0)
  const parsedOutput = JSON.parse(output)
  expect(Object.keys(parsedOutput)).toMatchSnapshot('found-license-types')
  const packagesWithMIT = parsedOutput['MIT']
  expect(packagesWithMIT.length).toBeGreaterThan(0)
  expect(Object.keys(packagesWithMIT[0])).toEqual(['name', 'version', 'path', 'license', 'vendorName', 'vendorUrl'])
  packagesWithMIT.forEach((pkg: { name: string, version: string, path: string }) => {
    const expectedPkgPath = path.join(workspaceDir, `${pkg.name}@${pkg.version}`.replace('/', '+'), 'node_modules', pkg.name)
    expect(pkg.path).toBe(expectedPkgPath)
  })
})

test('pnpm licenses: show details', async () => {
  await expect(
    licenses.handler({
      ...LICENSES_OPTIONS,
      dir: path.resolve('./test/fixtures/invalid'),
      long: true,
    })
  ).rejects.toThrowErrorMatchingInlineSnapshot(
    '"No pnpm-lock.yaml found: Cannot check a project without a lockfile"'
  )
})
