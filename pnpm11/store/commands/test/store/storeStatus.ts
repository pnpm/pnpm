import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import type { PnpmError } from '@pnpm/error'
import { prepare } from '@pnpm/prepare'
import { store } from '@pnpm/store.commands'
import { REGISTRY_MOCK_PORT } from '@pnpm/testing.registry-mock'
import { rimrafSync } from '@zkochan/rimraf'
import { safeExeca as execa } from 'execa'
import { temporaryDirectory } from 'tempy'

const REGISTRY = `http://localhost:${REGISTRY_MOCK_PORT}/npm/`
const pnpmBin = path.join(import.meta.dirname, '../../../../pnpm/bin/pnpm.mjs')

// Use an empty config dir to ensure the subprocess is not affected by
// the user's global pnpm config (e.g. enable-global-virtual-store).
const cleanConfigDir = temporaryDirectory()
const execaOpts = { env: { XDG_CONFIG_HOME: cleanConfigDir } }

test.each(['', 'virtualStoreDir: custom-links\n'])('store status checks global virtual store packages (%s)', async (settings) => {
  prepare()
  fs.writeFileSync('pnpm-workspace.yaml', `enableGlobalVirtualStore: true\n${settings}`)
  const storeDir = path.join(temporaryDirectory(), 'store')
  const options = [`--store-dir=${storeDir}`, `--registry=${REGISTRY}`]
  await execa('node', [
    pnpmBin, 'add', 'is-odd@3.0.1', '@pnpm.e2e/abc@1.0.0',
    '@pnpm.e2e/generated-bins@1.0.0',
    '--allow-build=@pnpm.e2e/generated-bins', ...options,
  ], execaOpts)
  expect(fs.existsSync('node_modules/@pnpm.e2e/generated-bins/bin/cmd1')).toBe(true)
  await execa('node', [pnpmBin, 'store', 'status', ...options], execaOpts)

  fs.writeFileSync('node_modules/@pnpm.e2e/abc/package.json', '{}\n')
  const result = await execa('node', [pnpmBin, 'store', 'status', ...options], { ...execaOpts, reject: false })
  expect(result.exitCode).toBe(1)
  expect(result.stderr).toContain('ERR_PNPM_MODIFIED_DEPENDENCY')
  expect(result.stderr).toContain('@pnpm.e2e/abc@1.0.0(')
  expect(result.stderr).not.toContain('is-odd@3.0.1')

  fs.unlinkSync('node_modules/.pnpm/lock.yaml')
  const withoutInstalledLockfile = await execa('node', [pnpmBin, 'store', 'status', ...options], { ...execaOpts, reject: false })
  expect(withoutInstalledLockfile.exitCode).toBe(1)
  expect(withoutInstalledLockfile.stderr).toContain('ERR_PNPM_MODIFIED_DEPENDENCY')
  expect(withoutInstalledLockfile.stderr).toContain('@pnpm.e2e/abc@1.0.0(')

  rimrafSync('node_modules/@pnpm.e2e/abc')
  fs.mkdirSync('node_modules/@pnpm.e2e/abc')
  const redirectedPeer = await execa('node', [pnpmBin, 'store', 'status', ...options], { ...execaOpts, reject: false })
  expect(redirectedPeer.exitCode).toBe(1)
  expect(redirectedPeer.stderr).toContain('ERR_PNPM_STORE_STATUS_UNVERIFIABLE')
})

test.each([
  ['changed approvals', '@pnpm.e2e/generated-bins@1.0.0', '--allow-build=@pnpm.e2e/generated-bins'],
  ['excluded optional dependencies', '@pnpm.e2e/pkg-with-good-optional', '--no-optional'],
])('store status uses the installed global graph (%s)', async (_scenario, dependency, installOption) => {
  prepare()
  fs.writeFileSync('pnpm-workspace.yaml', 'enableGlobalVirtualStore: true\n')
  const storeDir = path.join(temporaryDirectory(), 'store')
  const options = [`--store-dir=${storeDir}`, `--registry=${REGISTRY}`]
  await execa('node', [pnpmBin, 'add', dependency, installOption, ...options], execaOpts)
  fs.writeFileSync('pnpm-workspace.yaml', 'enableGlobalVirtualStore: true\nallowBuilds: {}\n')
  await execa('node', [pnpmBin, 'store', 'status', ...options], execaOpts)
})

test('store status reports an unavailable global slot separately from modified contents', async () => {
  prepare()
  fs.writeFileSync('pnpm-workspace.yaml', 'enableGlobalVirtualStore: true\n')
  const storeDir = path.join(temporaryDirectory(), 'store')
  const options = [`--store-dir=${storeDir}`, `--registry=${REGISTRY}`]
  await execa('node', [pnpmBin, 'add', 'is-odd@3.0.1', ...options], execaOpts)
  rimrafSync(fs.realpathSync('node_modules/is-odd'))
  const result = await execa('node', [pnpmBin, 'store', 'status', ...options], { ...execaOpts, reject: false })
  expect(result.exitCode).toBe(1)
  expect(result.stderr).toContain('ERR_PNPM_STORE_STATUS_UNVERIFIABLE')
  expect(result.stderr).not.toContain('ERR_PNPM_MODIFIED_DEPENDENCY')
})

test('store status rejects a different project slot after build policy changes', async () => {
  prepare()
  fs.writeFileSync('pnpm-workspace.yaml', 'enableGlobalVirtualStore: true\n')
  const storeDir = path.join(temporaryDirectory(), 'store')
  const options = [`--store-dir=${storeDir}`, `--registry=${REGISTRY}`]
  await execa('node', [pnpmBin, 'add', 'is-odd@3.0.1', ...options], execaOpts)
  fs.mkdirSync('other')
  fs.writeFileSync('other/package.json', '{}')
  fs.writeFileSync('other/pnpm-workspace.yaml', 'enableGlobalVirtualStore: true\ndangerouslyAllowAllBuilds: true\n')
  await execa('node', [pnpmBin, 'add', 'is-odd@3.0.1', ...options], { ...execaOpts, cwd: path.resolve('other') })
  fs.writeFileSync('pnpm-workspace.yaml', 'enableGlobalVirtualStore: true\ndangerouslyAllowAllBuilds: true\n')
  fs.unlinkSync('node_modules/is-odd/index.js')
  fs.writeFileSync('node_modules/is-odd/index.js', 'module.exports = () => false')
  const result = await execa('node', [pnpmBin, 'store', 'status', ...options], { ...execaOpts, reject: false })
  expect(result.exitCode).toBe(1)
  expect(result.stderr).toContain('ERR_PNPM_STORE_STATUS_UNVERIFIABLE')
})

test.each(['modulesDir: custom_modules\n', 'dedupeDirectDeps: true\n'])('store status checks workspace links (%s)', async (settings) => {
  prepare({ name: 'root-project', dependencies: { 'is-odd': '3.0.1' } })
  fs.writeFileSync('pnpm-workspace.yaml', `enableGlobalVirtualStore: true\npackages:\n  - packages/*\n${settings}`)
  fs.mkdirSync('packages/child', { recursive: true })
  fs.writeFileSync('packages/child/package.json', JSON.stringify({ name: 'child-project', dependencies: { 'is-odd': '3.0.1' } }))
  const options = [`--store-dir=${path.join(temporaryDirectory(), 'store')}`, `--registry=${REGISTRY}`]
  await execa('node', [pnpmBin, 'install', ...options], execaOpts)
  const result = await execa('node', [pnpmBin, 'store', 'status', ...options], { ...execaOpts, reject: false })
  expect(result.exitCode).toBe(0)
})

test('store status rejects escaping importer paths', async () => {
  prepare()
  fs.writeFileSync('pnpm-workspace.yaml', 'enableGlobalVirtualStore: true\n')
  const options = [`--store-dir=${path.join(temporaryDirectory(), 'store')}`, `--registry=${REGISTRY}`]
  await execa('node', [pnpmBin, 'add', 'is-odd@3.0.1', ...options], execaOpts)
  const lockfile = 'node_modules/.pnpm/lock.yaml'
  const yaml = fs.readFileSync(lockfile, 'utf8')
  expect(yaml).toContain('\n  .:')
  fs.writeFileSync(lockfile, yaml.replace('\n  .:', '\n  ../outside:'))
  const result = await execa('node', [pnpmBin, 'store', 'status', ...options], { ...execaOpts, reject: false })
  expect(result.exitCode).toBe(1)
  expect(result.stderr).toContain('ERR_PNPM_INVALID_DEPENDENCY_NAME')
})

test('CLI fails when store status finds modified packages', async () => {
  prepare()
  const tmp = temporaryDirectory()
  const cacheDir = path.join(tmp, 'cache')
  const storeDir = path.join(tmp, 'store')

  await execa('node', [
    pnpmBin,
    'add',
    'is-positive@3.1.0',
    `--store-dir=${storeDir}`,
    `--registry=${REGISTRY}`,
    '--verify-store-integrity',
  ], execaOpts)

  rimrafSync('node_modules/.pnpm/is-positive@3.1.0/node_modules/is-positive/index.js')

  let err!: PnpmError & { modified: string[] }
  try {
    await store.handler({
      cacheDir,
      dir: process.cwd(),
      pnpmHomeDir: '',
      configByUri: {},
      registriesByScope: { default: REGISTRY },
      storeDir,
      dlxCacheMaxAge: 0,
      virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
    }, ['status'])
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }
  expect(err.code).toBe('ERR_PNPM_MODIFIED_DEPENDENCY')
  expect(err.modified).toHaveLength(1)
  expect(err.modified[0]).toMatch(/is-positive/)
})

test('CLI does not fail when store status does not find modified packages', async () => {
  prepare()
  fs.writeFileSync('pnpm-workspace.yaml', 'allowBuilds: { "es5-ext": false, "fsevents": true }', 'utf8')
  const tmp = temporaryDirectory()
  const cacheDir = path.join(tmp, 'cache')
  const storeDir = path.join(tmp, 'store')

  await execa('node', [
    pnpmBin,
    `--store-dir=${storeDir}`,
    `--registry=${REGISTRY}`,
    '--verify-store-integrity',
    'add',
    'eslint@3.4.0',
    'gulp@4.0.2',
    'highcharts@5.0.10',
    'is-positive@3.1.0',
    'react@15.4.1',
    'webpack@5.24.2',
    'koorchik/node-mole-rpc',
  ], execaOpts)
  // store status does not fail on not installed optional dependencies
  await execa('node', [
    pnpmBin,
    'add',
    'not-compatible-with-any-os',
    '--save-optional',
    `--store-dir=${storeDir}`,
    `--registry=${REGISTRY}`,
    '--verify-store-integrity',
  ], execaOpts)

  await store.handler({
    cacheDir,
    dir: process.cwd(),
    pnpmHomeDir: '',
    configByUri: {},
    registriesByScope: { default: REGISTRY },
    storeDir,
    dlxCacheMaxAge: 0,
    virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
  }, ['status'])
})

test('CLI does not fail when storeDir is relative', async () => {
  prepare()
  const cacheDir = path.resolve('cache')
  const workspaceDir = process.cwd()
  const subpackageDir = path.join(workspaceDir, 'packages', 'foo')
  const relativeStoreDir = '../.store'

  fs.writeFileSync('pnpm-workspace.yaml', `packages:
  - "packages/*"
storeDir: "${relativeStoreDir}"
`, 'utf8')

  // make subpackage
  fs.mkdirSync(subpackageDir, { recursive: true })
  fs.writeFileSync(
    path.join(subpackageDir, 'package.json'),
    JSON.stringify({ name: 'foo', version: '0.0.0' })
  )

  await execa('node', [
    pnpmBin,
    'add',
    'is-positive@3.1.0',
    `--registry=${REGISTRY}`,
    '--verify-store-integrity',
  ], execaOpts)


  // relativeStoreDir should resolve from workspaceDir, not dir
  await store.handler({
    cacheDir,
    dir: subpackageDir,
    workspaceDir,
    pnpmHomeDir: '',
    configByUri: {},
    registriesByScope: { default: REGISTRY },
    storeDir: relativeStoreDir,
    dlxCacheMaxAge: 0,
    virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
  }, ['status'])
})

test('store status does not falsely report package with postinstall script as modified', async () => {
  prepare()
  const tmp = temporaryDirectory()
  const cacheDir = path.join(tmp, 'cache')
  const storeDir = path.join(tmp, 'store')

  await execa('node', [
    pnpmBin,
    'add',
    '@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0',
    '--allow-build=@pnpm.e2e/pre-and-postinstall-scripts-example',
    `--store-dir=${storeDir}`,
    `--registry=${REGISTRY}`,
    '--verify-store-integrity',
  ], execaOpts)

  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')).toBeTruthy()

  await store.handler({
    cacheDir,
    dir: process.cwd(),
    pnpmHomeDir: '',
    configByUri: {},
    registriesByScope: { default: REGISTRY },
    storeDir,
    dlxCacheMaxAge: 0,
    virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
  }, ['status'])
})

test('hardlinked built package with modified file is reported as modified', async () => {
  prepare()
  const tmp = temporaryDirectory()
  const cacheDir = path.join(tmp, 'cache')
  const storeDir = path.join(tmp, 'store')

  await execa('node', [
    pnpmBin,
    'add',
    '@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0',
    '--allow-build=@pnpm.e2e/pre-and-postinstall-scripts-example',
    `--store-dir=${storeDir}`,
    `--registry=${REGISTRY}`,
    '--verify-store-integrity',
  ], execaOpts)

  const pkgDir = 'node_modules/.pnpm/@pnpm.e2e+pre-and-postinstall-scripts-example@1.0.0/node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example'
  const targetFile = path.join(pkgDir, 'package.json')
  const externalFile = path.join(process.cwd(), 'dummy-link')
  fs.copyFileSync(targetFile, externalFile)
  fs.unlinkSync(targetFile)
  fs.linkSync(externalFile, targetFile)
  fs.writeFileSync(targetFile, '{"name": "modified"}', 'utf8')

  let err!: PnpmError & { modified: string[] }
  try {
    await store.handler({
      cacheDir,
      dir: process.cwd(),
      pnpmHomeDir: '',
      configByUri: {},
      registriesByScope: { default: REGISTRY },
      storeDir,
      dlxCacheMaxAge: 0,
      virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
    }, ['status'])
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }
  expect(err?.code).toBe('ERR_PNPM_MODIFIED_DEPENDENCY')
})

test('symlinked package file in built package is reported as modified', async () => {
  prepare()
  const tmp = temporaryDirectory()
  const cacheDir = path.join(tmp, 'cache')
  const storeDir = path.join(tmp, 'store')

  await execa('node', [
    pnpmBin,
    'add',
    '@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0',
    '--allow-build=@pnpm.e2e/pre-and-postinstall-scripts-example',
    `--store-dir=${storeDir}`,
    `--registry=${REGISTRY}`,
    '--verify-store-integrity',
  ], execaOpts)

  const pkgDir = 'node_modules/.pnpm/@pnpm.e2e+pre-and-postinstall-scripts-example@1.0.0/node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example'
  const targetFile = path.join(pkgDir, 'package.json')
  const externalFile = path.join(process.cwd(), 'dummy-symlink-target')
  fs.copyFileSync(targetFile, externalFile)
  fs.unlinkSync(targetFile)
  fs.symlinkSync(externalFile, targetFile)
  fs.writeFileSync(externalFile, '{"name": "modified"}', 'utf8')

  let err!: PnpmError & { modified: string[] }
  try {
    await store.handler({
      cacheDir,
      dir: process.cwd(),
      pnpmHomeDir: '',
      configByUri: {},
      registriesByScope: { default: REGISTRY },
      storeDir,
      dlxCacheMaxAge: 0,
      virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
    }, ['status'])
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }
  expect(err?.code).toBe('ERR_PNPM_MODIFIED_DEPENDENCY')
})

test('removes reporter listener on early return when lockfile is missing', async () => {
  prepare()
  const { logger } = await import('@pnpm/logger')
  const reportedMessages: unknown[] = []
  const reporter = (logObj: unknown) => {
    reportedMessages.push((logObj as { message?: unknown }).message)
  }

  const { storeStatus } = await import('../../src/store/storeStatus/index.js')
  await storeStatus({
    reporter,
    dir: process.cwd(),
    storeDir: temporaryDirectory(),
    virtualStoreDirMaxLength: 120,
  })

  logger.info({ message: 'logged after the store status', prefix: process.cwd() })
  await new Promise((resolve) => setTimeout(resolve, 50))

  expect(reportedMessages).not.toContain('logged after the store status')
})


