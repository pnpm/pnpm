import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { temporaryDirectory } from 'tempy'

import { makeRunPacquet } from '../lib/runPacquet.js'

function setupPacquetConfigDep (version: string | undefined, packageName: 'pacquet' | '@pnpm/pacquet' = 'pacquet'): string {
  const lockfileDir = temporaryDirectory()
  const pkgDir = path.join(lockfileDir, 'node_modules/.pnpm-config', packageName)
  fs.mkdirSync(pkgDir, { recursive: true })
  fs.writeFileSync(
    path.join(pkgDir, 'package.json'),
    JSON.stringify(version == null ? { name: packageName } : { name: packageName, version })
  )
  return lockfileDir
}

function makeEngine (lockfileDir: string, packageName: 'pacquet' | '@pnpm/pacquet' = 'pacquet'): ReturnType<typeof makeRunPacquet> {
  return makeRunPacquet({
    lockfileDir,
    packageName,
    argv: { original: [], remain: [] },
    isInstallCommand: true,
    virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
  })
}

test.each([
  ['0.11.7', true],
  ['0.11.7-rc.1', true],
  ['0.12.3', true],
  ['1.0.0', true],
  ['0.11.6', false],
  ['0.11.0', false],
  ['0.10.99', false],
  ['0.2.2', false],
  ['0.0.1', false],
])('pacquet %s -> supportsResolution %s', (version, expected) => {
  const engine = makeEngine(setupPacquetConfigDep(version))
  expect(engine.supportsResolution).toBe(expected)
})

test('supportsResolution is false when the pacquet version cannot be read', () => {
  const engine = makeEngine(setupPacquetConfigDep(undefined))
  expect(engine.supportsResolution).toBe(false)
})

test('supportsResolution is false when the pacquet config dependency is absent', () => {
  const engine = makeEngine(temporaryDirectory())
  expect(engine.supportsResolution).toBe(false)
})

test('the version is read from the @pnpm/pacquet scoped alias too', () => {
  const lockfileDir = setupPacquetConfigDep('0.11.7', '@pnpm/pacquet')
  expect(makeEngine(lockfileDir, '@pnpm/pacquet').supportsResolution).toBe(true)
})
