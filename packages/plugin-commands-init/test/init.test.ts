import path from 'path'
import { init } from '@pnpm/plugin-commands-init'
import prepare, { prepareEmpty } from '@pnpm/prepare'
import { sync as loadJsonFile } from 'load-json-file'

test('init a new package.json', async () => {
  prepareEmpty()
  await init.handler({ dir: process.cwd() })
  const manifest = loadJsonFile(path.resolve('package.json'))
  expect(manifest).toBeTruthy()
})

test('throws an error if a package.json exists in the current directory', async () => {
  prepare({})

  await expect(
    init.handler({ dir: process.cwd() })
  ).rejects.toThrow('package.json already exists')
})
