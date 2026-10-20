import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { getTrustedHoistRoot, prepareWorkspaceModulesDir, safeJoinWorkspaceModulesDir, symlinkDependency, symlinkDependencySync, symlinkDirectRootDependency, validateWorkspaceModulesDir } from '@pnpm/fs.symlink-dependency'
import { tempDir } from '@pnpm/prepare'

const escapeAliases = [
  '@x/../../../etc',
  '../sibling',
  '',
  '.',
  // Reserved names that resolve *inside* `node_modules` but would
  // overwrite pnpm-owned layout, so the containment check alone can't
  // catch them.
  '.bin',
  '.pnpm',
  'node_modules',
]

test.each(escapeAliases)('symlinkDependency refuses alias %p', async (alias) => {
  const tmp = tempDir(false)
  const destModulesDir = path.join(tmp, 'node_modules')
  fs.mkdirSync(destModulesDir)
  await expect(
    symlinkDependency(path.join(tmp, 'dep'), destModulesDir, alias)
  ).rejects.toThrow(expect.objectContaining({ code: 'ERR_PNPM_INVALID_DEPENDENCY_NAME' }))
})

test.each(escapeAliases)('symlinkDependencySync refuses alias %p', (alias) => {
  const tmp = tempDir(false)
  const destModulesDir = path.join(tmp, 'node_modules')
  fs.mkdirSync(destModulesDir)
  expect(() => {
    symlinkDependencySync(path.join(tmp, 'dep'), destModulesDir, alias)
  }).toThrow(expect.objectContaining({ code: 'ERR_PNPM_INVALID_DEPENDENCY_NAME' }))
})

test.each(escapeAliases)('symlinkDirectRootDependency refuses alias %p', async (alias) => {
  const tmp = tempDir(false)
  const destModulesDir = path.join(tmp, 'node_modules')
  fs.mkdirSync(destModulesDir)
  await expect(symlinkDirectRootDependency(path.join(tmp, 'dep'), destModulesDir, alias, {
    linkedPackage: { name: 'dep', version: '1.0.0' },
    prefix: '',
  })).rejects.toThrow(expect.objectContaining({ code: 'ERR_PNPM_INVALID_DEPENDENCY_NAME' }))
})

const validAliases = ['foo', '@scope/name', 'foo.bar']

test.each(validAliases)('symlinkDependency accepts valid alias %p', async (alias) => {
  const tmp = tempDir(false)
  const destModulesDir = path.join(tmp, 'node_modules')
  fs.mkdirSync(destModulesDir)
  const dep = path.join(tmp, 'dep')
  fs.mkdirSync(dep)
  await expect(symlinkDependency(dep, destModulesDir, alias)).resolves.toBeDefined()
  expect(fs.existsSync(path.join(destModulesDir, alias))).toBe(true)
})

test.each(['foo', '@scope/name', 'packages/pkg-a', 'nested/inner'])('safeJoinWorkspaceModulesDir accepts alias %p', (alias) => {
  const modulesDir = path.join(tempDir(false), 'node_modules')
  expect(safeJoinWorkspaceModulesDir(modulesDir, alias)).toBe(path.join(modulesDir, alias))
})

test.each(['', '.', '..', '../outside', 'packages/../outside', 'packages//outside', '/absolute', '..\\outside', 'C:\\outside', '.pnpm/foo', '.bin/foo', 'node_modules/foo'])('safeJoinWorkspaceModulesDir refuses alias %p', (alias) => {
  const modulesDir = path.join(tempDir(false), 'node_modules')
  expect(() => safeJoinWorkspaceModulesDir(modulesDir, alias)).toThrow(expect.objectContaining({ code: 'ERR_PNPM_INVALID_DEPENDENCY_NAME' }))
})

test('prepareWorkspaceModulesDir refuses a symlinked destination parent', async () => {
  const tmp = tempDir(false)
  const modulesDir = path.join(tmp, 'node_modules')
  const outside = path.join(tmp, 'outside')
  fs.mkdirSync(modulesDir)
  fs.mkdirSync(outside)
  await fs.promises.symlink(outside, path.join(modulesDir, 'nested'), process.platform === 'win32' ? 'junction' : 'dir')

  await expect(prepareWorkspaceModulesDir(modulesDir, 'nested/inner')).rejects.toMatchObject({
    code: 'ERR_PNPM_INVALID_DEPENDENCY_NAME',
  })
  expect(fs.existsSync(path.join(outside, 'inner'))).toBe(false)
})

test('prepareWorkspaceModulesDir accepts a symlinked modules root pointing to a real directory', async () => {
  const tmp = tempDir(false)
  const modulesDir = path.join(tmp, 'node_modules')
  const outside = path.join(tmp, 'outside')
  fs.mkdirSync(outside)
  await fs.promises.symlink(outside, modulesDir, process.platform === 'win32' ? 'junction' : 'dir')

  await expect(prepareWorkspaceModulesDir(modulesDir, 'project')).resolves.toBe(path.join(modulesDir, 'project'))
  expect(fs.statSync(modulesDir).isDirectory()).toBe(true)
})

test('prepareWorkspaceModulesDir refuses a symlinked modules root pointing to a file', async () => {
  const tmp = tempDir(false)
  const modulesDir = path.join(tmp, 'node_modules')
  const outsideFile = path.join(tmp, 'file.txt')
  fs.writeFileSync(outsideFile, 'hello')
  await fs.promises.symlink(outsideFile, modulesDir, 'file')

  await expect(prepareWorkspaceModulesDir(modulesDir, 'project')).rejects.toMatchObject({
    code: 'ERR_PNPM_INVALID_DEPENDENCY_NAME',
  })
})

test('prepareWorkspaceModulesDir creates a fresh multi-component modules root', async () => {
  const tmp = tempDir(false)
  const modulesDir = path.join(tmp, 'fresh/nested/node_modules')

  await expect(prepareWorkspaceModulesDir(modulesDir, 'project')).resolves.toBe(path.join(modulesDir, 'project'))
  expect(fs.statSync(modulesDir).isDirectory()).toBe(true)
})

test('validateWorkspaceModulesDir refuses a symlinked existing parent without creating missing parents', async () => {
  const tmp = tempDir(false)
  const modulesDir = path.join(tmp, 'node_modules')
  const outside = path.join(tmp, 'outside')
  fs.mkdirSync(modulesDir)
  fs.mkdirSync(outside)
  await fs.promises.symlink(outside, path.join(modulesDir, 'nested'), process.platform === 'win32' ? 'junction' : 'dir')

  await expect(validateWorkspaceModulesDir(modulesDir, 'nested/inner')).rejects.toMatchObject({
    code: 'ERR_PNPM_INVALID_DEPENDENCY_NAME',
  })
  await expect(validateWorkspaceModulesDir(modulesDir, 'missing/inner')).resolves.toBe(path.join(modulesDir, 'missing/inner'))
  expect(fs.existsSync(path.join(modulesDir, 'missing'))).toBe(false)
})

test('validateWorkspaceModulesDir accepts a symlinked modules root pointing to a real directory', async () => {
  const tmp = tempDir(false)
  const modulesDir = path.join(tmp, 'node_modules')
  const outside = path.join(tmp, 'outside')
  fs.mkdirSync(outside)
  await fs.promises.symlink(outside, modulesDir, process.platform === 'win32' ? 'junction' : 'dir')

  await expect(validateWorkspaceModulesDir(modulesDir, 'project')).resolves.toBe(path.join(modulesDir, 'project'))
  expect(fs.statSync(modulesDir).isDirectory()).toBe(true)
})

test('validateWorkspaceModulesDir refuses a symlinked modules root pointing to a file', async () => {
  const tmp = tempDir(false)
  const modulesDir = path.join(tmp, 'node_modules')
  const outsideFile = path.join(tmp, 'file.txt')
  fs.writeFileSync(outsideFile, 'hello')
  await fs.promises.symlink(outsideFile, modulesDir, 'file')

  await expect(validateWorkspaceModulesDir(modulesDir, 'project')).rejects.toMatchObject({
    code: 'ERR_PNPM_INVALID_DEPENDENCY_NAME',
  })
})

test('prepareWorkspaceModulesDir refuses a symlinked ancestor below the trusted root', async () => {
  const tmp = tempDir(false)
  const trustedRoot = path.join(tmp, 'node_modules')
  const modulesDir = path.join(trustedRoot, '.pnpm/node_modules')
  const outside = path.join(tmp, 'outside')
  fs.mkdirSync(trustedRoot)
  fs.mkdirSync(outside)
  await fs.promises.symlink(outside, path.join(trustedRoot, '.pnpm'), process.platform === 'win32' ? 'junction' : 'dir')

  await expect(prepareWorkspaceModulesDir(modulesDir, 'project', trustedRoot)).rejects.toMatchObject({
    code: 'ERR_PNPM_INVALID_DEPENDENCY_NAME',
  })
  expect(fs.existsSync(path.join(outside, 'node_modules/project'))).toBe(false)
})

test('prepareWorkspaceModulesDir refuses a dangling symlinked modules root', async () => {
  const tmp = tempDir(false)
  const modulesDir = path.join(tmp, 'node_modules')
  const missing = path.join(tmp, 'missing')
  await fs.promises.symlink(missing, modulesDir, process.platform === 'win32' ? 'junction' : 'dir')

  await expect(prepareWorkspaceModulesDir(modulesDir, 'project')).rejects.toMatchObject({
    code: 'ERR_PNPM_INVALID_DEPENDENCY_NAME',
  })
  expect(fs.existsSync(missing)).toBe(false)
})

test('getTrustedHoistRoot is the public hoist directory for a modules directory inside it', () => {
  const tmp = tempDir(false)
  const publicDir = path.join(tmp, 'node_modules')
  expect(getTrustedHoistRoot(publicDir, publicDir)).toBe(publicDir)
  expect(getTrustedHoistRoot(publicDir, path.join(publicDir, '.pnpm/node_modules'))).toBe(publicDir)
})

test('getTrustedHoistRoot is the modules directory itself outside the public hoist directory', () => {
  const tmp = tempDir(false)
  const publicDir = path.join(tmp, 'project/node_modules')
  const externalStoreModulesDir = path.join(tmp, 'store/node_modules')
  expect(getTrustedHoistRoot(publicDir, externalStoreModulesDir)).toBe(externalStoreModulesDir)
  expect(getTrustedHoistRoot(publicDir, path.join(tmp, 'project/node_modules_2'))).toBe(path.join(tmp, 'project/node_modules_2'))
})
