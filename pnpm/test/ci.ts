import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { prepare, tempDir } from '@pnpm/prepare'

import { execPnpmSync } from './utils/index.js'

test('pnpm ci fails when lockfile is missing', () => {
  tempDir()
  fs.writeFileSync('package.json', JSON.stringify({
    name: 'test-ci-no-lockfile',
    version: '1.0.0',
    dependencies: {
      'is-positive': '1.0.0',
    },
  }))

  const result = execPnpmSync(['ci'])
  expect(result.status).not.toBe(0)
})

test('pnpm ci removes node_modules and installs from lockfile', () => {
  prepare({
    name: 'test-ci-clean-install',
    version: '1.0.0',
    dependencies: {
      'is-positive': '1.0.0',
    },
  })

  // First, do a normal install to create lockfile
  execPnpmSync(['install'])

  // Create a marker file in node_modules to verify it gets deleted
  const markerPath = path.join(process.cwd(), 'node_modules', 'ci-test-marker')
  fs.writeFileSync(markerPath, 'test')

  // Run ci
  const result = execPnpmSync(['ci'])
  expect(result.status).toBe(0)

  // Verify marker file is gone (node_modules was cleaned)
  expect(fs.existsSync(markerPath)).toBe(false)

  // Verify dependencies are installed
  expect(fs.existsSync(path.join(process.cwd(), 'node_modules', 'is-positive'))).toBe(true)
})

test('pnpm ci fails when package.json conflicts with lockfile', () => {
  prepare({
    name: 'test-ci-conflict',
    version: '1.0.0',
    dependencies: {
      'is-positive': '1.0.0',
    },
  })

  // First, do a normal install
  execPnpmSync(['install'])

  // Modify package.json to create conflict
  const manifestPath = path.join(process.cwd(), 'package.json')
  const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8')) as { dependencies: Record<string, string> }
  manifest.dependencies['is-negative'] = '1.0.0'
  fs.writeFileSync(manifestPath, JSON.stringify(manifest, null, 2))

  // ci should fail because lockfile doesn't have is-negative
  const result = execPnpmSync(['ci'])
  expect(result.status).not.toBe(0)
})
