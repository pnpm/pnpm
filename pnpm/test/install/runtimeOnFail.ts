import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { prepare } from '@pnpm/prepare'

import { execPnpm } from '../utils/index.js'

test('runtimeOnFail=download causes Node.js to be downloaded even when the manifest does not set onFail', async () => {
  const project = prepare({
    devEngines: {
      runtime: {
        name: 'node',
        version: '24.0.0',
      },
    },
  })
  fs.writeFileSync('pnpm-workspace.yaml', 'runtimeOnFail: download\n', 'utf8')

  await execPnpm(['install'])

  project.isExecutable('.bin/node')
  const lockfile = project.readLockfile()
  expect(lockfile.importers['.'].devDependencies).toStrictEqual({
    node: {
      specifier: 'runtime:24.0.0',
      version: 'runtime:24.0.0',
    },
  })
})

test('runtimeOnFail=ignore prevents Node.js download even when manifest sets onFail=download', async () => {
  const project = prepare({
    devEngines: {
      runtime: {
        name: 'node',
        version: '24.0.0',
        onFail: 'download',
      },
    },
  })
  fs.writeFileSync('pnpm-workspace.yaml', 'runtimeOnFail: ignore\n', 'utf8')

  await execPnpm(['install'])

  const lockfile = project.readLockfile()
  expect(lockfile.importers['.'].devDependencies).toBeUndefined()
})

test('--no-runtime keeps the runtime entry in the lockfile but skips installing the binary', async () => {
  const project = prepare({
    devEngines: {
      runtime: {
        name: 'node',
        version: '24.0.0',
        onFail: 'download',
      },
    },
  })

  await execPnpm(['install'])
  project.isExecutable('.bin/node')
  const lockfileBefore = project.readLockfile()
  expect(lockfileBefore.importers['.'].devDependencies).toStrictEqual({
    node: { specifier: 'runtime:24.0.0', version: 'runtime:24.0.0' },
  })

  fs.rmSync('node_modules', { recursive: true, force: true })
  await execPnpm(['install', '--frozen-lockfile', '--no-runtime'])

  const lockfileAfter = project.readLockfile()
  expect(lockfileAfter.importers['.'].devDependencies).toStrictEqual({
    node: { specifier: 'runtime:24.0.0', version: 'runtime:24.0.0' },
  })
  const nodeBin = path.join('node_modules', '.bin', process.platform === 'win32' ? 'node.cmd' : 'node')
  expect(fs.existsSync(nodeBin)).toBe(false)
})
