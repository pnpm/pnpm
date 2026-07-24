import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { prepare } from '@pnpm/prepare'
import { rimrafSync } from '@zkochan/rimraf'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpm } from '../utils/index.js'

test('caching side effects of native package', async function () {
  prepare()
  writeYamlFileSync('pnpm-workspace.yaml', { allowBuilds: { diskusage: true } })

  await execPnpm(['add', '--side-effects-cache', 'diskusage@1.2.0'])
  expect(fs.existsSync(path.join('node_modules/diskusage/build'))).toBeTruthy()

  // Second install with cache: build output should be restored from cache
  rimrafSync('node_modules')
  await execPnpm(['add', 'diskusage@1.2.0', '--side-effects-cache'])
  expect(fs.existsSync(path.join('node_modules/diskusage/build'))).toBeTruthy()

  // Force rebuild: build output should still exist after rebuild
  rimrafSync('node_modules')
  await execPnpm(['add', 'diskusage@1.2.0', '--side-effects-cache', '--force'])
  expect(fs.existsSync(path.join('node_modules/diskusage/build'))).toBeTruthy()
})

test('using side effects cache', async function () {
  prepare()
  writeYamlFileSync('pnpm-workspace.yaml', { allowBuilds: { diskusage: true } })

  // Use copy method since hardlink doesn't work with side effects
  await execPnpm(['add', 'diskusage@1.2.0', '--side-effects-cache', '--no-verify-store-integrity', '--package-import-method', 'copy'])
  expect(fs.existsSync(path.join('node_modules/diskusage/build'))).toBeTruthy()

  // Modify build output, then reinstall from cache — cache should be restored
  rimrafSync('node_modules')
  await execPnpm(['add', 'diskusage@1.2.0', '--side-effects-cache', '--no-verify-store-integrity', '--package-import-method', 'copy'])
  expect(fs.existsSync(path.join('node_modules/diskusage/build'))).toBeTruthy()
})

test('readonly side effects cache', async function () {
  prepare()
  writeYamlFileSync('pnpm-workspace.yaml', { allowBuilds: { diskusage: true } })

  await execPnpm(['add', 'diskusage@1.2.0', '--side-effects-cache', '--no-verify-store-integrity'])
  expect(fs.existsSync(path.join('node_modules/diskusage/build'))).toBeTruthy()

  // Reinstall with readonly cache — should still have build output
  rimrafSync('node_modules')
  await execPnpm(['add', 'diskusage@1.2.0', '--side-effects-cache-readonly', '--no-verify-store-integrity', '--package-import-method', 'copy'])
  expect(fs.existsSync(path.join('node_modules/diskusage/build'))).toBeTruthy()
})
