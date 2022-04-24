import path from 'path'
import { init } from '@pnpm/plugin-commands-init'
import prepare, { prepareEmpty } from '@pnpm/prepare'
import { sync as loadJsonFile } from 'load-json-file'

test('init a new package.json', async () => {
  prepareEmpty()
  await init.handler({ rawConfig: {} })
  const manifest = loadJsonFile(path.resolve('package.json'))
  expect(manifest).toBeTruthy()
})

test('throws an error if a package.json exists in the current directory', async () => {
  prepare({})

  await expect(
    init.handler({ rawConfig: {} })
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
  await init.handler({ rawConfig })
  const manifest: Record<string, string> = loadJsonFile(path.resolve('package.json'))
  const expectAuthor = `${rawConfig['init-author-name']} <${rawConfig['init-author-email']}> (${rawConfig['init-author-url']})`
  expect(manifest.version).toBe(rawConfig['init-version'])
  expect(manifest.author).toBe(expectAuthor)
  expect(manifest.license).toBe(rawConfig['init-license'])
})
