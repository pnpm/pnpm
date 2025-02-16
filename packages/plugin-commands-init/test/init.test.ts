import path from 'path'
import fs from 'fs'
import { init } from '@pnpm/plugin-commands-init'
import { prepare, prepareEmpty } from '@pnpm/prepare'
import { type ProjectManifest } from '@pnpm/types'
import { sync as loadJsonFile } from 'load-json-file'

test('init a new package.json', async () => {
  prepareEmpty()
  await init.handler({ rawConfig: {}, cliOptions: {} })
  const manifest = loadJsonFile(path.resolve('package.json'))
  expect(manifest).toBeTruthy()
})

test('throws an error if a package.json exists in the current directory', async () => {
  prepare({})

  await expect(
    init.handler({ rawConfig: {}, cliOptions: {} })
  ).rejects.toThrow('package.json already exists')
})

test('init a new package.json with npmrc', async () => {
  const rawConfig = {
    'init-author-email': 'xxxxxx@pnpm.com',
    'init-author-name': 'pnpm',
    'init-author-url': 'https://www.github.com/pnpm',
    'init-license': 'MIT',
    'init-version': '2.0.0',
  }
  prepareEmpty()
  await init.handler({ rawConfig, cliOptions: {} })
  const manifest: Record<string, string> = loadJsonFile(path.resolve('package.json'))
  const expectAuthor = `${rawConfig['init-author-name']} <${rawConfig['init-author-email']}> (${rawConfig['init-author-url']})`
  expect(manifest.version).toBe(rawConfig['init-version'])
  expect(manifest.author).toBe(expectAuthor)
  expect(manifest.license).toBe(rawConfig['init-license'])
})

test('throw an error if params are passed to the init command', async () => {
  prepare({})

  await expect(
    init.handler({ rawConfig: {}, cliOptions: {} }, ['react-app'])
  ).rejects.toThrow('init command does not accept any arguments')
})

test('init a new package.json if a package.json exists in the parent directory', async () => {
  prepare({})
  fs.mkdirSync('empty-dir1')
  process.chdir('./empty-dir1')

  await init.handler({ rawConfig: {}, cliOptions: {} })
  const manifest = loadJsonFile(path.resolve('package.json'))
  expect(manifest).toBeTruthy()
})

test('init a new package.json if a package.json exists in the current directory but specifies --dir option', async () => {
  prepare({})
  fs.mkdirSync('empty-dir2')

  await init.handler({
    rawConfig: {},
    cliOptions: {
      dir: './empty-dir2',
    },
  })
  const manifest = loadJsonFile(path.resolve('empty-dir2/package.json'))
  expect(manifest).toBeTruthy()
})

test('init a new package.json with init-package-manager=true', async () => {
  prepareEmpty()
  await init.handler({ rawConfig: { 'init-package-manager': true }, cliOptions: {}, initPackageManager: true })
  const manifest = loadJsonFile<ProjectManifest>(path.resolve('package.json'))
  expect(manifest).toBeTruthy()
  expect(manifest.packageManager).toBeTruthy()
})

test('init a new package.json with init-package-manager=false', async () => {
  prepareEmpty()
  await init.handler({ rawConfig: { 'init-package-manager': false }, cliOptions: {}, initPackageManager: false })
  const manifest = loadJsonFile<ProjectManifest>(path.resolve('package.json'))
  expect(manifest).toBeTruthy()
  expect(manifest).not.toHaveProperty('packageManager')
})
