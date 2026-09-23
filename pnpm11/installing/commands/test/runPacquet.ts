import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { temporaryDirectory } from 'tempy'

import { DEV_PREINSTALL_ALREADY_RAN_ENV, makePacquetEnv, makeRunPacquet, type MakeRunPacquetOpts, ROOT_PREINSTALL_ALREADY_RAN_ENV } from '../lib/runPacquet.js'

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
    forceIgnoresPlatform: true,
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

const envOpts: MakeRunPacquetOpts = {
  lockfileDir: '/does-not-need-to-exist',
  packageName: 'pacquet',
  argv: { original: [], remain: [] },
  isInstallCommand: true,
  virtualStoreDirMaxLength: 120,
  forceIgnoresPlatform: true,
}

test('the effective forceIgnoresPlatform value reaches pacquet through the environment', () => {
  const previous = process.env.pnpm_config_force_ignores_platform
  process.env.pnpm_config_force_ignores_platform = 'false'
  try {
    const env = makePacquetEnv(envOpts)
    expect(env.PNPM_CONFIG_FORCE_IGNORES_PLATFORM).toBe('true')
    expect(env.pnpm_config_force_ignores_platform).toBeUndefined()
    expect(makePacquetEnv({ ...envOpts, forceIgnoresPlatform: false }).PNPM_CONFIG_FORCE_IGNORES_PLATFORM).toBe('false')
  } finally {
    if (previous == null) {
      delete process.env.pnpm_config_force_ignores_platform
    } else {
      process.env.pnpm_config_force_ignores_platform = previous
    }
  }
})

// Resolve mode hands pacquet the whole install after pnpm has already run
// the root `pnpm:devPreinstall`, and is the one delegation shape that
// passes no flag pacquet could key off instead.
test(`${DEV_PREINSTALL_ALREADY_RAN_ENV} is set only when delegating a resolving install`, () => {
  expect(makePacquetEnv(envOpts, { resolve: true })[DEV_PREINSTALL_ALREADY_RAN_ENV]).toBe('true')
  expect(makePacquetEnv(envOpts)[DEV_PREINSTALL_ALREADY_RAN_ENV]).toBeUndefined()
  expect(makePacquetEnv(envOpts, { filterResolvedProgress: true })[DEV_PREINSTALL_ALREADY_RAN_ENV]).toBeUndefined()
})

// Whether pnpm ran the root's preinstall depends on the command, not on
// the delegation shape, so the marker follows the call rather than
// `resolve`.
test(`${ROOT_PREINSTALL_ALREADY_RAN_ENV} is set only when pnpm ran the root preinstall`, () => {
  expect(makePacquetEnv(envOpts, { rootProjectPreinstallRan: true })[ROOT_PREINSTALL_ALREADY_RAN_ENV]).toBe('true')
  expect(makePacquetEnv(envOpts, { resolve: true, rootProjectPreinstallRan: true })[ROOT_PREINSTALL_ALREADY_RAN_ENV]).toBe('true')
  expect(makePacquetEnv(envOpts, { rootProjectPreinstallRan: false })[ROOT_PREINSTALL_ALREADY_RAN_ENV]).toBeUndefined()
  expect(makePacquetEnv(envOpts, { resolve: true })[ROOT_PREINSTALL_ALREADY_RAN_ENV]).toBeUndefined()
  expect(makePacquetEnv(envOpts)[ROOT_PREINSTALL_ALREADY_RAN_ENV]).toBeUndefined()
})

// An ambient value would otherwise suppress a hook on a delegation where
// pnpm relies on pacquet running it. The lowercase spelling is a distinct
// key on POSIX but the same one on Windows, where pacquet would read it
// as a delegation marker.
test.each([
  DEV_PREINSTALL_ALREADY_RAN_ENV,
  DEV_PREINSTALL_ALREADY_RAN_ENV.toLowerCase(),
  ROOT_PREINSTALL_ALREADY_RAN_ENV,
  ROOT_PREINSTALL_ALREADY_RAN_ENV.toLowerCase(),
])('an inherited %s never leaks into a delegation that did not set it', (key) => {
  const previous = process.env[key]
  process.env[key] = 'true'
  try {
    const env = makePacquetEnv(envOpts)
    const survivor = Object.keys(env).find(
      (name) => name.toLowerCase() === key.toLowerCase()
    )
    expect(survivor).toBeUndefined()
  } finally {
    if (previous == null) {
      delete process.env[key]
    } else {
      process.env[key] = previous
    }
  }
})
