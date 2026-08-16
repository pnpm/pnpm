import fs from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import { expect, test } from '@jest/globals'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import type { PnpmError } from '@pnpm/error'
import { addDependenciesToPackage, install } from '@pnpm/installing.deps-installer'
import type { Modules } from '@pnpm/installing.modules-yaml'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import type { ProjectRootDir } from '@pnpm/types'
import { rimrafSync } from '@zkochan/rimraf'
import { isCI } from 'ci-info'

import { validateModules } from '../src/install/validateModules.js'
import { testDefaults } from './utils/index.js'

test('fail on non-compatible node_modules', async () => {
  prepareEmpty()
  const opts = testDefaults()

  await saveModulesYaml('0.50.0', opts.storeDir)

  try {
    await addDependenciesToPackage({}, ['is-negative'], opts)
    throw new Error('should have failed')
  } catch (err: any) { // eslint-disable-line
    expect(err.code).toBe('ERR_PNPM_MODULES_BREAKING_CHANGE')
  }
})

test("don't fail on non-compatible node_modules when forced", async () => {
  prepareEmpty()
  const opts = testDefaults({ force: true })

  await saveModulesYaml('0.50.0', opts.storeDir)

  await install({}, opts)
})

test('forced modules repair never purges the project root', async () => {
  prepareEmpty()
  const sentinel = path.resolve('keep.txt')
  fs.writeFileSync(sentinel, 'keep')
  fs.writeFileSync('.modules.yaml', 'packageManager: pnpm@3\nlayoutVersion: 1\n')

  await expect(install({}, testDefaults({
    confirmModulesPurge: false,
    force: true,
    modulesDir: '.',
  }))).rejects.toMatchObject({ code: 'ERR_PNPM_UNSAFE_MODULES_DIR' })

  expect(fs.readFileSync(sentinel, 'utf8')).toBe('keep')
})

test('forced modules repair never follows a modules directory symlink', async () => {
  prepareEmpty()
  const projectRoot = process.cwd() as ProjectRootDir
  const modulesDir = path.join(projectRoot, 'linked-modules')
  const sentinel = path.join(projectRoot, 'keep.txt')
  fs.writeFileSync(sentinel, 'keep')
  fs.symlinkSync(projectRoot, modulesDir, process.platform === 'win32' ? 'junction' : 'dir')
  const opts = testDefaults()

  try {
    await expect(validateModules({ virtualStoreDirMaxLength: 1 } as Modules, [{
      id: '.',
      modulesDir,
      rootDir: projectRoot,
    }], {
      confirmModulesPurge: false,
      forceNewModules: true,
      lockfileDir: projectRoot,
      modulesDir,
      registriesByScope: opts.registriesByScope,
      storeDir: opts.storeDir,
      virtualStoreDir: path.join(modulesDir, '.pnpm'),
      virtualStoreDirMaxLength: 2,
    })).rejects.toMatchObject({ code: 'ERR_PNPM_UNSAFE_MODULES_DIR' })

    expect(fs.readFileSync(sentinel, 'utf8')).toBe('keep')
  } finally {
    fs.unlinkSync(modulesDir)
  }
})

test("don't fail on non-compatible node_modules when forced in a workspace", async () => {
  preparePackages([
    {
      location: 'pkg',
      package: {},
    },
  ])
  const opts = testDefaults({ force: true })

  process.chdir('pkg')
  const { updatedManifest: manifest } = await addDependenciesToPackage({}, ['is-positive@1.0.0'], testDefaults({ lockfileDir: path.resolve('..') }))
  rimrafSync('node_modules')

  process.chdir('..')

  fs.writeFileSync('node_modules/.modules.yaml', `packageManager: pnpm@${3}\nstore: ${opts.storeDir}\nlayoutVersion: 1`)

  await install(manifest, { ...opts, dir: path.resolve('pkg'), lockfileDir: process.cwd() })
})

test('do not fail on non-compatible node_modules when forced with a named installation', async () => {
  prepareEmpty()
  const opts = testDefaults()

  await saveModulesYaml('0.50.0', opts.storeDir)

  let err!: PnpmError
  try {
    await addDependenciesToPackage({}, ['is-negative'], opts)
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }
  expect(err.code).toBe('ERR_PNPM_MODULES_BREAKING_CHANGE')

  await install({}, {
    ...opts,
    confirmModulesPurge: false,
  })
})

test("don't fail on non-compatible store when forced", async () => {
  prepareEmpty()
  const opts = testDefaults({ force: true })

  await saveModulesYaml('0.32.0', opts.storeDir)

  await install({}, opts)
})

test('do not fail on non-compatible store when forced during named installation', async () => {
  prepareEmpty()
  const opts = testDefaults()

  await saveModulesYaml('0.32.0', opts.storeDir)

  let err!: PnpmError
  try {
    await addDependenciesToPackage({}, ['is-negative'], opts)
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }
  expect(err.code).toBe('ERR_PNPM_MODULES_BREAKING_CHANGE')

  await install({}, {
    ...opts,
    confirmModulesPurge: false,
  })
})

test('fail fast with actionable hint on non-TTY when modules purge needs confirmation', async () => {
  prepareEmpty()
  const opts = testDefaults()

  await saveModulesYaml('0.50.0', opts.storeDir)

  const originalIsTTY = process.stdin.isTTY
  Object.defineProperty(process.stdin, 'isTTY', { value: false, configurable: true })

  let err: unknown
  try {
    await install({}, opts)
  } catch (_err: unknown) {
    err = _err
  } finally {
    Object.defineProperty(process.stdin, 'isTTY', { value: originalIsTTY, configurable: true })
  }

  expect(util.types.isNativeError(err)).toBeTruthy()
  if (util.types.isNativeError(err)) {
    expect('code' in err && err.code).toBe('ERR_PNPM_ABORTED_REMOVE_MODULES_DIR_NO_TTY')
    expect(err.message).toContain('no TTY')
    expect('hint' in err && typeof err.hint === 'string' && err.hint).toContain('confirmModulesPurge')
  }
})

async function saveModulesYaml (pnpmVersion: string, storeDir: string) {
  fs.mkdirSync('node_modules')
  fs.writeFileSync('node_modules/.modules.yaml', `packageManager: pnpm@${pnpmVersion}\nstoreDir: ${storeDir}`)
}

test(`fail on non-compatible ${WANTED_LOCKFILE} when frozen lockfile installation is used`, async () => {
  if (isCI) {
    console.log('this test will always fail on CI servers')
    return
  }

  prepareEmpty()
  fs.writeFileSync(WANTED_LOCKFILE, '')

  try {
    await addDependenciesToPackage({}, ['is-negative'], testDefaults({ frozenLockfile: true }))
    throw new Error('should have failed')
  } catch (err: any) { // eslint-disable-line
    if (err.message === 'should have failed') throw err
    expect(err.code).toBe('ERR_PNPM_BROKEN_LOCKFILE')
  }
})

test(`don't fail on non-compatible ${WANTED_LOCKFILE} when forced`, async () => {
  prepareEmpty()
  fs.writeFileSync(WANTED_LOCKFILE, '')

  await addDependenciesToPackage({}, ['is-negative'], testDefaults({ force: true }))
})
