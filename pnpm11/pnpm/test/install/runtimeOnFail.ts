import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { prepare } from '@pnpm/prepare'
import { writeYamlFileSync } from 'write-yaml-file'

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
  expectNoNodeBin()
})

test('--no-runtime works on a fresh checkout with no lockfile (non-frozen path)', async () => {
  const project = prepare({
    devEngines: {
      runtime: {
        name: 'node',
        version: '24.0.0',
        onFail: 'download',
      },
    },
  })

  await execPnpm(['install', '--no-runtime'])

  const lockfile = project.readLockfile()
  expect(lockfile.importers['.'].devDependencies).toStrictEqual({
    node: { specifier: 'runtime:24.0.0', version: 'runtime:24.0.0' },
  })
  expectNoNodeBin()
})

test('--no-runtime works with enableGlobalVirtualStore=true', async () => {
  const project = prepare({
    devEngines: {
      runtime: {
        name: 'node',
        version: '24.0.0',
        onFail: 'download',
      },
    },
  })
  writeYamlFileSync(path.resolve('pnpm-workspace.yaml'), {
    enableGlobalVirtualStore: true,
    storeDir: path.resolve('store'),
  })

  await execPnpm(['install', '--no-runtime'])

  const lockfile = project.readLockfile()
  expect(lockfile.importers['.'].devDependencies).toStrictEqual({
    node: { specifier: 'runtime:24.0.0', version: 'runtime:24.0.0' },
  })
  expectNoNodeBin()
})

function expectNoNodeBin (): void {
  const binDir = path.join('node_modules', '.bin')
  for (const name of ['node', 'node.exe', 'node.cmd', 'node.ps1']) {
    const p = path.join(binDir, name)
    // lstatSync (vs existsSync) catches dangling symlinks too — existsSync
    // follows symlinks and would return false for a symlink whose target was
    // never created, hiding a real bug.
    let exists = false
    try {
      fs.lstatSync(p)
      exists = true
    } catch {}
    expect(exists).toBe(false)
  }
}
