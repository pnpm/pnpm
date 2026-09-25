import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { ignoredBuilds } from '@pnpm/building.commands'
import { writeModulesManifest } from '@pnpm/installing.modules-yaml'
import { tempDir } from '@pnpm/prepare-temp-dir'
import type { DepPath } from '@pnpm/types'

const DEFAULT_MODULES_MANIFEST = {
  hoistedDependencies: {},
  layoutVersion: 4,
  packageManager: '',
  included: {
    optionalDependencies: true,
    dependencies: true,
    devDependencies: true,
  },
  pendingBuilds: [],
  prunedAt: '',
  skipped: [],
  storeDir: '',
  virtualStoreDir: '',
  virtualStoreDirMaxLength: 90,
  registries: {
    default: '',
  },
}

test('ignoredBuilds lists automatically ignored dependencies', async () => {
  const dir = tempDir()
  const modulesDir = path.join(dir, 'node_modules')
  fs.mkdirSync(modulesDir, { recursive: true })
  await writeModulesManifest(modulesDir, {
    ...DEFAULT_MODULES_MANIFEST,
    ignoredBuilds: new Set(['foo@1.0.0' as DepPath]),
  })
  const output = await ignoredBuilds.handler({
    dir,
    modulesDir,
    allowBuilds: {},
  })
  expect(output).toMatchSnapshot()
})

test('ignoredBuilds lists explicitly ignored dependencies', async () => {
  const dir = tempDir()
  const modulesDir = path.join(dir, 'node_modules')
  fs.mkdirSync(modulesDir, { recursive: true })
  await writeModulesManifest(modulesDir, {
    ...DEFAULT_MODULES_MANIFEST,
    ignoredBuilds: new Set(),
  })
  const output = await ignoredBuilds.handler({
    dir,
    modulesDir,
    allowBuilds: { bar: false },
  })
  expect(output).toMatchSnapshot()
})

test('ignoredBuilds lists both automatically and explicitly ignored dependencies', async () => {
  const dir = tempDir()
  const modulesDir = path.join(dir, 'node_modules')
  fs.mkdirSync(modulesDir, { recursive: true })
  await writeModulesManifest(modulesDir, {
    ...DEFAULT_MODULES_MANIFEST,
    ignoredBuilds: new Set(['foo@1.0.0', 'bar@1.0.0'] as DepPath[]),
  })
  const output = await ignoredBuilds.handler({
    dir,
    modulesDir,
    allowBuilds: { qar: false, zoo: false },
  })
  expect(output).toMatchSnapshot()
})

test('ignoredBuilds prints an info message when there is no node_modules', async () => {
  const dir = tempDir()
  const modulesDir = path.join(dir, 'node_modules')
  const output = await ignoredBuilds.handler({
    dir,
    modulesDir,
    allowBuilds: { qar: false, zoo: false },
  })
  expect(output).toMatchSnapshot()
})
