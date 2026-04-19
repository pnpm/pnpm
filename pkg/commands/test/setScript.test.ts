import fs from 'node:fs'
import path from 'node:path'
import { setScript } from '../src/index'
import { prepare } from '@pnpm/prepare'
import { readPackageJsonFromDirRawSync } from '@pnpm/pkg-manifest.reader'

test('set-script adds a script to package.json', async () => {
  const project = prepare()

  await setScript.handler({ dir: process.cwd() }, ['test-script', 'echo "hello"'])

  const manifest = readPackageJsonFromDirRawSync(process.cwd())
  expect(manifest.scripts?.['test-script']).toBe('echo "hello"')
})

test('set-script updates an existing script in package.json', async () => {
  const project = prepare({
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
