import fs from 'node:fs'
import path from 'node:path'

import { install } from '@pnpm/installing.commands'
import { prepare } from '@pnpm/prepare'
import { readYamlFileSync } from 'read-yaml-file'

import * as ci from '../src/ci.js'
import { DEFAULT_OPTS } from './utils/index.js'

test('pnpm ci fails when lockfile is missing', async () => {
  prepare({
    name: 'test-ci-no-lockfile',
    version: '1.0.0',
    dependencies: {
      'is-positive': '1.0.0',
    },
  })

  await expect(
    ci.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
      lockfileDir: process.cwd(),
    })
  ).rejects.toThrow('Cannot perform a clean install because the lockfile is missing')
})

test('pnpm ci removes node_modules and installs from lockfile', async () => {
  const project = prepare({
    name: 'test-ci-clean-install',
    version: '1.0.0',
    dependencies: {
      'is-positive': '1.0.0',
    },
  })

  // First, do a normal install to create lockfile
  await install.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    lockfileDir: process.cwd(),
  })

  // Read the lockfile before ci
  const lockfileBefore = readYamlFileSync<Record<string, unknown>>(path.join(process.cwd(), 'pnpm-lock.yaml'))

  // Create a marker file in node_modules to verify it gets deleted
  const markerPath = path.join(process.cwd(), 'node_modules', '.ci-test-marker')
  fs.writeFileSync(markerPath, 'test')

  // Run ci
  await ci.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    lockfileDir: process.cwd(),
  })

  // Verify marker file is gone (node_modules was deleted)
  expect(fs.existsSync(markerPath)).toBe(false)

  // Verify lockfile was not modified
  const lockfileAfter = readYamlFileSync<Record<string, unknown>>(path.join(process.cwd(), 'pnpm-lock.yaml'))
  expect(lockfileAfter).toEqual(lockfileBefore)

  // Verify dependencies are installed
  project.has('is-positive')
})

test('pnpm ci fails when package.json conflicts with lockfile', async () => {
  prepare({
    name: 'test-ci-conflict',
    version: '1.0.0',
    dependencies: {
      'is-positive': '1.0.0',
    },
  })

  // First, do a normal install
  await install.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    lockfileDir: process.cwd(),
  })

  // Modify package.json to create conflict
  const manifestPath = path.join(process.cwd(), 'package.json')
  const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8')) as { dependencies: Record<string, string> }
  manifest.dependencies['is-negative'] = '1.0.0' // Add new dependency
  fs.writeFileSync(manifestPath, JSON.stringify(manifest, null, 2))

  // ci should fail because lockfile doesn't have is-negative
  await expect(
    ci.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
      lockfileDir: process.cwd(),
    })
  ).rejects.toThrow() // Will throw frozen lockfile error
})
