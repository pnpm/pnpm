import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { prepare, preparePackages, tempDir } from '@pnpm/prepare'
import { writeYamlFileSync } from 'write-yaml-file'

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

test('pnpm ci reinstalls workspace package node_modules', () => {
  preparePackages([
    {
      name: 'foo',
      version: '0.0.0',
      private: true,
      dependencies: { 'is-positive': '1.0.0' },
    },
  ])

  fs.writeFileSync('package.json', JSON.stringify({ name: 'root', version: '1.0.0', private: true }))
  writeYamlFileSync('pnpm-workspace.yaml', { packages: ['*'] })

  expect(execPnpmSync(['install']).status).toBe(0)
  expect(fs.existsSync(path.join('foo', 'node_modules', 'is-positive'))).toBe(true)

  const result = execPnpmSync(['ci'])
  expect(result.status).toBe(0)
  expect(fs.existsSync(path.join('foo', 'node_modules', 'is-positive'))).toBe(true)
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

test.each([
  '--ignore-scripts',
  '--config.ignore-scripts=true',
])('pnpm ci %s does not run lifecycle scripts when clean script is present', (flag) => {
  prepare({
    name: 'test-ci-ignore-scripts',
    version: '1.0.0',
    scripts: {
      clean: 'node -e "process.exit(1)"',
      install: 'node -e "process.exit(2)"',
    },
  })

  execPnpmSync(['install', '--lockfile-only'])

  const result = execPnpmSync(['ci', flag])
  expect(result.status).toBe(0)
})


test.each([
  '--lockfile',
  '--config.lockfile=true',
])('pnpm ci %s keeps the lockfile and installs from it', (flag) => {
  prepare({
    name: 'test-ci-lockfile-flag',
    version: '1.0.0',
    dependencies: {
      'is-positive': '1.0.0',
    },
  })

  expect(execPnpmSync(['install']).status).toBe(0)
  const lockfile = fs.readFileSync('pnpm-lock.yaml', 'utf8')

  const result = execPnpmSync(['ci', flag])
  expect(result.status).toBe(0)
  expect(fs.readFileSync('pnpm-lock.yaml', 'utf8')).toBe(lockfile)
  expect(fs.existsSync(path.join('node_modules', 'is-positive'))).toBe(true)
})

test('pnpm ci does not delete files in external modulesDir', () => {
  for (const useAbsolute of [false, true]) {
    prepare({
      name: 'test-ci-external-modules-dir',
      version: '1.0.0',
      dependencies: {
        'is-positive': '1.0.0',
      },
    })
    expect(execPnpmSync(['install', '--lockfile-only']).status).toBe(0)

    const externalTarget = path.resolve('..', 'external-target')
    fs.mkdirSync(path.join(externalTarget, 'existing-pkg'), { recursive: true })
    fs.writeFileSync(path.join(externalTarget, 'keep.txt'), 'keep')
    fs.writeFileSync(path.join(externalTarget, 'existing-pkg', 'package.json'), '{}')
    writeYamlFileSync('pnpm-workspace.yaml', {
      modulesDir: useAbsolute ? externalTarget : '../external-target',
    })

    const result = execPnpmSync(['ci'])
    expect(result.status).toBe(0)
    expect(fs.existsSync(path.join(externalTarget, 'keep.txt'))).toBe(true)
    expect(fs.existsSync(path.join(externalTarget, 'existing-pkg', 'package.json'))).toBe(true)
    expect(fs.existsSync(path.join(externalTarget, 'is-positive'))).toBe(true)
  }
})

test('pnpm ci unlinks symlinked node_modules without deleting target contents and installs into a real directory', () => {
  prepare({
    name: 'test-ci-symlinked-node-modules',
    version: '1.0.0',
    dependencies: {
      'is-positive': '1.0.0',
    },
  })
  expect(execPnpmSync(['install', '--lockfile-only']).status).toBe(0)

  const externalTarget = path.resolve('..', 'external-target')
  fs.mkdirSync(path.join(externalTarget, '.pnpm'), { recursive: true })
  fs.mkdirSync(path.join(externalTarget, 'lodash'), { recursive: true })
  fs.writeFileSync(path.join(externalTarget, 'keep.txt'), 'keep')
  fs.writeFileSync(path.join(externalTarget, 'lodash', 'package.json'), '{}')
  fs.symlinkSync(externalTarget, 'node_modules', process.platform === 'win32' ? 'junction' : 'dir')

  const result = execPnpmSync(['ci'])
  expect(result.status).toBe(0)
  expect(fs.existsSync(path.join(externalTarget, 'keep.txt'))).toBe(true)
  expect(fs.existsSync(path.join(externalTarget, '.pnpm'))).toBe(true)
  expect(fs.existsSync(path.join(externalTarget, 'lodash', 'package.json'))).toBe(true)
  expect(fs.lstatSync('node_modules').isSymbolicLink()).toBe(false)
  expect(fs.lstatSync('node_modules').isDirectory()).toBe(true)
  expect(fs.existsSync(path.join('node_modules', 'is-positive'))).toBe(true)
})
