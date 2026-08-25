import fs from 'node:fs'
import path from 'node:path'

import { beforeEach, expect, jest, test } from '@jest/globals'
import { packageManager } from '@pnpm/cli.meta'
import { prepareEmpty } from '@pnpm/prepare'
import type { ProjectManifest } from '@pnpm/types'
import { loadJsonFileSync } from 'load-json-file'
import semver from 'semver'

jest.unstable_mockModule('@pnpm/engine.pm.commands', () => ({
  isReleaseInstallable: jest.fn(),
  resolvePnpmVersion: jest.fn(),
}))
const { isReleaseInstallable, resolvePnpmVersion } = await import('@pnpm/engine.pm.commands')
const { init } = await import('@pnpm/workspace.commands')

const mockResolvePnpmVersion = jest.mocked(resolvePnpmVersion)
const mockIsReleaseInstallable = jest.mocked(isReleaseInstallable)

const NEWER_VERSION = semver.inc(packageManager.version, 'major')!
// A prerelease of the running version is lower than it whatever version this
// build reports, including the `0.0.0` an unreleased checkout carries.
const OLDER_VERSION = `${packageManager.version}-0`

beforeEach(() => {
  mockResolvePnpmVersion.mockReset()
  mockIsReleaseInstallable.mockReset().mockReturnValue(true)
})

async function initPinned (): Promise<ProjectManifest> {
  await init.handler({
    cacheDir: path.resolve('cache'),
    cliOptions: {},
    initPackageManager: true,
  })
  return loadJsonFileSync<ProjectManifest>(path.resolve('package.json'))
}

test('pins the version the "latest" tag resolves to', async () => {
  prepareEmpty()
  mockResolvePnpmVersion.mockResolvedValue({ version: NEWER_VERSION })

  const manifest = await initPinned()

  expect(mockResolvePnpmVersion).toHaveBeenCalledWith(expect.anything(), 'latest')
  expect(manifest.packageManager).toBe(`pnpm@${NEWER_VERSION}`)
  expect(manifest.devEngines?.packageManager).toEqual({
    name: 'pnpm',
    version: NEWER_VERSION,
    onFail: 'download',
  })
})

test('does not pin a "latest" that is older than the running pnpm', async () => {
  prepareEmpty()
  mockResolvePnpmVersion.mockResolvedValue({ version: OLDER_VERSION })

  const manifest = await initPinned()

  expect(manifest.packageManager).toBe(`pnpm@${packageManager.version}`)
})

test('pins the running pnpm when the lookup fails', async () => {
  prepareEmpty()
  mockResolvePnpmVersion.mockRejectedValue(new Error('getaddrinfo ENOTFOUND registry.npmjs.org'))

  const manifest = await initPinned()

  expect(manifest.packageManager).toBe(`pnpm@${packageManager.version}`)
})

test('pins the running pnpm when "latest" resolves to nothing', async () => {
  prepareEmpty()
  mockResolvePnpmVersion.mockResolvedValue(undefined)

  const manifest = await initPinned()

  expect(manifest.packageManager).toBe(`pnpm@${packageManager.version}`)
})

test('pins the running pnpm when "latest" violates the release policy', async () => {
  prepareEmpty()
  mockResolvePnpmVersion.mockResolvedValue({
    version: NEWER_VERSION,
    policyViolation: {
      code: 'MINIMUM_RELEASE_AGE_VIOLATION',
      name: 'pnpm',
      version: NEWER_VERSION,
      reason: 'is too new',
      resolution: {
        integrity: 'sha512-',
        tarball: `https://registry.npmjs.org/pnpm/-/pnpm-${NEWER_VERSION}.tgz`,
      },
    },
  })

  const manifest = await initPinned()

  expect(manifest.packageManager).toBe(`pnpm@${packageManager.version}`)
})

// Which releases are broken is `@pnpm/engine.pm.commands`' business and is
// tested there; what matters here is that init refuses to pin one. The pin is
// shared but the wrapper is not, so pinning a release the running wrapper
// happens to survive would still break every teammate on the other wrapper.
test('pins the running pnpm when "latest" is not an installable release', async () => {
  prepareEmpty()
  mockResolvePnpmVersion.mockResolvedValue({ version: NEWER_VERSION })
  mockIsReleaseInstallable.mockReturnValue(false)

  const manifest = await initPinned()

  expect(mockIsReleaseInstallable).toHaveBeenCalledWith(NEWER_VERSION)
  expect(manifest.packageManager).toBe(`pnpm@${packageManager.version}`)
})

test('does not overwrite a package.json created while the pin was being resolved', async () => {
  prepareEmpty()
  const manifestPath = path.resolve('package.json')
  const writtenMeanwhile = { name: 'written-by-someone-else' }
  mockResolvePnpmVersion.mockImplementation(async () => {
    fs.writeFileSync(manifestPath, JSON.stringify(writtenMeanwhile))
    return { version: NEWER_VERSION }
  })

  await expect(initPinned()).rejects.toThrow('package.json already exists')

  expect(loadJsonFileSync(manifestPath)).toStrictEqual(writtenMeanwhile)
})

test('does not reach the registry when offline', async () => {
  prepareEmpty()
  await init.handler({
    cacheDir: path.resolve('cache'),
    cliOptions: {},
    initPackageManager: true,
    offline: true,
  })
  const manifest = loadJsonFileSync<ProjectManifest>(path.resolve('package.json'))

  expect(mockResolvePnpmVersion).not.toHaveBeenCalled()
  expect(manifest.packageManager).toBe(`pnpm@${packageManager.version}`)
})
