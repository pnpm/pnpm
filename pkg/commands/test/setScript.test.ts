import { expect, test } from '@jest/globals'
import { readPackageJsonFromDirRawSync } from '@pnpm/pkg-manifest.reader'
import { prepare } from '@pnpm/prepare'
import { setScript } from '../src/index.js'

test('set-script adds a script to package.json', async () => {
  prepare()

  await setScript.handler({ dir: process.cwd() }, ['test-script', 'echo "hello"'])

  const manifest = readPackageJsonFromDirRawSync(process.cwd())
  expect(manifest.scripts?.['test-script']).toBe('echo "hello"')
})

test('set-script updates an existing script in package.json', async () => {
  prepare({
    scripts: {
      'test-script': 'echo "old"',
    },
  })

  await setScript.handler({ dir: process.cwd() }, ['test-script', 'echo "new"'])

  const manifest = readPackageJsonFromDirRawSync(process.cwd())
  expect(manifest.scripts?.['test-script']).toBe('echo "new"')
})

test('set-script throws error if missing arguments', async () => {
  prepare()

  await expect(setScript.handler({ dir: process.cwd() }, ['test-script']))
    .rejects.toThrow('Missing script name or command')
})
