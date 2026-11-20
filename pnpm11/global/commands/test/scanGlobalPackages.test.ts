import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterEach, expect, test } from '@jest/globals'
import { readGlobalGroupDependencies, scanGlobalPackages } from '@pnpm/global.packages'
import { symlinkDir } from 'symlink-dir'

const tempDirs: string[] = []

function makeTempDir (): string {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-global-packages-test-'))
  tempDirs.push(dir)
  return dir
}

afterEach(() => {
  for (const dir of tempDirs.splice(0)) {
    fs.rmSync(dir, { recursive: true, force: true })
  }
})

test('readGlobalGroupDependencies reifies downloaded engines.runtime entries and filters invalid aliases', () => {
  const installDir = makeTempDir()
  fs.writeFileSync(
    path.join(installDir, 'package.json'),
    JSON.stringify({
      dependencies: {
        cowsay: '^1.5.0',
        '../escape': '1.0.0',
      },
      engines: {
        runtime: [
          { name: 'node', version: 'rc/22.0.0-rc.1', onFail: 'download' },
          { name: 'deno', version: '^1.40.0', onFail: 'error' },
        ],
      },
    }),
    'utf8'
  )

  expect(readGlobalGroupDependencies(installDir)).toStrictEqual({
    cowsay: '^1.5.0',
    node: 'runtime:rc/22.0.0-rc.1',
  })
})

test('scanGlobalPackages includes runtime-only global groups with downloaded runtimes', async () => {
  const globalDir = makeTempDir()
  const installDir = path.join(globalDir, '.install-runtime-only')
  fs.mkdirSync(installDir, { recursive: true })
  fs.writeFileSync(
    path.join(installDir, 'package.json'),
    JSON.stringify({
      engines: {
        runtime: { name: 'deno', version: '^1.40.0', onFail: 'download' },
      },
    }),
    'utf8'
  )
  await symlinkDir(installDir, path.join(globalDir, 'hash-deno'))

  const scanned = scanGlobalPackages(globalDir)
  expect(scanned).toStrictEqual([
    {
      hash: 'hash-deno',
      installDir: fs.realpathSync(installDir),
      dependencies: {
        deno: 'runtime:^1.40.0',
      },
    },
  ])
})
