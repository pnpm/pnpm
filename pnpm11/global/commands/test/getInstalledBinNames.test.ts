import { promises as fs } from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import util from 'node:util'

import { afterEach, expect, test } from '@jest/globals'
import { getInstalledBinNames, type GlobalPackageInfo } from '@pnpm/global.packages'

const fixtureRoots: string[] = []

afterEach(async () => {
  const roots = fixtureRoots.splice(0)
  await Promise.all(roots.map(assertFixtureRootContained))
  await Promise.all(roots.map(async (root) => fs.rm(root, { force: true, recursive: true })))
})

test('returns an empty complete result for a readable package without bins', async () => {
  const info = await createGlobalPackageInfo({ withoutBins: readableManifest('without-bins') })

  await expect(getInstalledBinNames(info)).resolves.toStrictEqual([])
})

test('rejects instead of treating one missing declared package as a complete empty result', async () => {
  const info = await createGlobalPackageInfo({ missing: null })

  await expectEnumerationToReject(info, (error) => {
    expect(getErrorCode(error)).toBe('ENOENT')
  })
})

test('rejects instead of returning known bins when another declared package is missing', async () => {
  const info = await createGlobalPackageInfo({
    known: readableManifest('known', { known: 'bin/known.js' }),
    missing: null,
  })
  await expect(getInstalledBinNames(withAliases(info, ['known']))).resolves.toStrictEqual(['known'])

  await expectEnumerationToReject(info, (error) => {
    expect(getErrorCode(error)).toBe('ENOENT')
  })

  await fs.writeFile(
    path.join(info.installDir, 'node_modules', 'missing', 'package.json'),
    readableManifest('missing', { unknown: 'bin/unknown.js' })
  )
  const repairedBinNames = await getInstalledBinNames(info)
  expect(repairedBinNames.sort()).toStrictEqual(['known', 'unknown'])
  const repeatedBinNames = await getInstalledBinNames(info)
  expect(repeatedBinNames.sort()).toStrictEqual(['known', 'unknown'])
})

test('rejects instead of returning known bins when another declared package has malformed JSON', async () => {
  const info = await createGlobalPackageInfo({
    known: readableManifest('known', { known: 'bin/known.js' }),
    malformed: '{',
  })
  await expect(getInstalledBinNames(withAliases(info, ['known']))).resolves.toStrictEqual(['known'])

  await expectEnumerationToReject(info, (error) => {
    expect(getErrorCode(error)).toBe('ERR_PNPM_BAD_PACKAGE_JSON')
  })
})

test('rejects instead of returning known bins after a real non-ENOENT filesystem read failure', async () => {
  const info = await createGlobalPackageInfo({
    known: readableManifest('known', { known: 'bin/known.js' }),
    unreadable: PACKAGE_JSON_DIRECTORY,
  })
  await expect(getInstalledBinNames(withAliases(info, ['known']))).resolves.toStrictEqual(['known'])

  await expectEnumerationToReject(info, (error) => {
    expect(util.types.isNativeError(error)).toBe(true)
    expect(getErrorCode(error)).toBeDefined()
    expect(getErrorCode(error)).not.toBe('ENOENT')
  })
})

const PACKAGE_JSON_DIRECTORY = Symbol('package.json directory')

type PackageJsonFixture = string | null | typeof PACKAGE_JSON_DIRECTORY

async function createGlobalPackageInfo (packages: Record<string, PackageJsonFixture>): Promise<GlobalPackageInfo> {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-global-bin-enumeration-'))
  await assertFixtureRootContained(root)
  fixtureRoots.push(root)
  const installDir = path.join(root, 'install')
  const dependencies = Object.fromEntries(Object.keys(packages).map((alias) => [alias, '1.0.0']))

  await Promise.all(Object.entries(packages).map(async ([alias, packageJson]) => {
    const packageDir = path.join(installDir, 'node_modules', alias)
    await fs.mkdir(packageDir, { recursive: true })
    if (packageJson === null) return
    const packageJsonPath = path.join(packageDir, 'package.json')
    if (packageJson === PACKAGE_JSON_DIRECTORY) {
      await fs.mkdir(packageJsonPath)
    } else {
      await fs.writeFile(packageJsonPath, packageJson)
    }
  }))

  return {
    dependencies,
    hash: 'fixture-hash',
    installDir,
  }
}

async function assertFixtureRootContained (root: string): Promise<void> {
  expect(root).not.toBe('')
  const rootStat = await fs.stat(root)
  expect(rootStat.isDirectory()).toBe(true)
  const [canonicalRoot, canonicalTempDir] = await Promise.all([
    fs.realpath(root),
    fs.realpath(os.tmpdir()),
  ])
  expect(path.dirname(canonicalRoot)).toBe(canonicalTempDir)
}

function readableManifest (name: string, bin?: Record<string, string>): string {
  return JSON.stringify({
    bin,
    name,
    version: '1.0.0',
  })
}

function withAliases (info: GlobalPackageInfo, aliases: string[]): GlobalPackageInfo {
  return {
    ...info,
    dependencies: Object.fromEntries(aliases.map((alias) => [alias, info.dependencies[alias]])),
  }
}

async function expectEnumerationToReject (
  info: GlobalPackageInfo,
  assertError: (error: unknown) => void
): Promise<void> {
  const outcome = await getInstalledBinNames(info).then(
    (bins) => ({ bins, kind: 'resolved' as const }),
    (error: unknown) => ({ error, kind: 'rejected' as const })
  )
  if (outcome.kind === 'resolved') {
    throw new Error(`Expected incomplete enumeration to reject, but it resolved with ${JSON.stringify(outcome.bins)}`)
  }
  assertError(outcome.error)
}

function getErrorCode (error: unknown): unknown {
  return typeof error === 'object' && error !== null && 'code' in error ? error.code : undefined
}
