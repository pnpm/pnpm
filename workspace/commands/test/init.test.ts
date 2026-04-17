import fs from 'node:fs'
import path from 'node:path'

import { prepare, prepareEmpty } from '@pnpm/prepare'
import type { ProjectManifest } from '@pnpm/types'
import { init } from '@pnpm/workspace.commands'
import { loadJsonFileSync } from 'load-json-file'

test('init a new package.json', async () => {
  prepareEmpty()
  await init.handler({ cliOptions: {} })
  const manifest = loadJsonFileSync(path.resolve('package.json'))
  expect(manifest).toBeTruthy()
})

test('throws an error if a package.json exists in the current directory', async () => {
  prepare({})

  await expect(
    init.handler({ cliOptions: {} })
  ).rejects.toThrow('package.json already exists')
})

test('init a new package.json with author and license settings', async () => {
  prepareEmpty()
  await init.handler({
    cliOptions: {},
    initAuthorEmail: 'xxxxxx@pnpm.com',
    initAuthorName: 'pnpm',
    initAuthorUrl: 'https://www.github.com/pnpm',
    initLicense: 'MIT',
    initVersion: '2.0.0',
  })
  const manifest: Record<string, string> = loadJsonFileSync(path.resolve('package.json'))
  expect(manifest.version).toBe('2.0.0')
  expect(manifest.author).toBe('pnpm <xxxxxx@pnpm.com> (https://www.github.com/pnpm)')
  expect(manifest.license).toBe('MIT')
})

test('throw an error if params are passed to the init command', async () => {
  prepare({})

  await expect(
    init.handler({ cliOptions: {} }, ['react-app'])
  ).rejects.toThrow('init command does not accept any arguments')
})

test('init a new package.json if a package.json exists in the parent directory', async () => {
  prepare({})
  fs.mkdirSync('empty-dir1')
  process.chdir('./empty-dir1')

  await init.handler({ cliOptions: {} })
  const manifest = loadJsonFileSync(path.resolve('package.json'))
  expect(manifest).toBeTruthy()
})

test('init a new package.json if a package.json exists in the current directory but specifies --dir option', async () => {
  prepare({})
  fs.mkdirSync('empty-dir2')

  await init.handler({
    cliOptions: {
      dir: './empty-dir2',
    },
  })
  const manifest = loadJsonFileSync(path.resolve('empty-dir2/package.json'))
  expect(manifest).toBeTruthy()
})

test('init a new package.json with init-package-manager=true', async () => {
  prepareEmpty()
  await init.handler({ cliOptions: {}, initPackageManager: true })
  const manifest = loadJsonFileSync<ProjectManifest>(path.resolve('package.json'))
  expect(manifest).toBeTruthy()
  expect(manifest).not.toHaveProperty('packageManager')
  expect(manifest.devEngines?.packageManager).toEqual({
    name: 'pnpm',
    version: expect.stringMatching(/^\^\d+\.\d+\.\d+/),
    onFail: 'download',
  })
})

test('init a new package.json with init-package-manager=false', async () => {
  prepareEmpty()
  await init.handler({ cliOptions: {}, initPackageManager: false })
  const manifest = loadJsonFileSync<ProjectManifest>(path.resolve('package.json'))
  expect(manifest).toBeTruthy()
  expect(manifest).not.toHaveProperty('packageManager')
  expect(manifest).not.toHaveProperty('devEngines')
})

test('init a new package.json with init-type=module', async () => {
  prepareEmpty()
  await init.handler({ cliOptions: {}, initType: 'module' })
  const manifest = loadJsonFileSync<ProjectManifest>(path.resolve('package.json'))
  expect(manifest.type).toBe('module')
})

test('init a new package.json with --bare', async () => {
  prepareEmpty()
  await init.handler({ cliOptions: {}, bare: true })
  const manifest = loadJsonFileSync<ProjectManifest>(path.resolve('package.json'))
  expect(manifest).not.toHaveProperty(['name'])
  expect(manifest).not.toHaveProperty(['version'])
  expect(manifest).not.toHaveProperty(['description'])
  expect(manifest).not.toHaveProperty(['main'])
  expect(manifest).not.toHaveProperty(['keywords'])
  expect(manifest).not.toHaveProperty(['author'])
  expect(manifest).not.toHaveProperty(['license'])
})
