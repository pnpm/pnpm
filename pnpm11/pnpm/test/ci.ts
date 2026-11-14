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

test('pnpm ci preserves existing node_modules when lockfile is missing or outdated', () => {
  prepare({
    name: 'test-ci-preserve-nm',
    version: '1.0.0',
    dependencies: {
      'is-positive': '1.0.0',
    },
  })

  expect(execPnpmSync(['install']).status).toBe(0)
  const markerPath = path.join('node_modules', 'keep-marker.txt')
  fs.writeFileSync(markerPath, 'keep')
  const savedLockfile = fs.readFileSync('pnpm-lock.yaml', 'utf8')

  // Missing pnpm-lock.yaml (even while node_modules/.pnpm/lock.yaml exists)
  fs.unlinkSync('pnpm-lock.yaml')
  const missingRes = execPnpmSync(['ci'])
  expect(missingRes.status).not.toBe(0)
  expect(missingRes.stdout.toString() + missingRes.stderr.toString()).toContain('ERR_PNPM_NO_LOCKFILE')
  expect(fs.existsSync(markerPath)).toBe(true)
  expect(fs.existsSync(path.join('node_modules', 'is-positive'))).toBe(true)

  // Outdated pnpm-lock.yaml
  fs.writeFileSync('pnpm-lock.yaml', savedLockfile)
  fs.writeFileSync('package.json', JSON.stringify({
    name: 'test-ci-preserve-nm',
    version: '1.0.0',
    dependencies: {
      'is-positive': '1.0.0',
      'is-negative': '1.0.0',
    },
  }))
  const outdatedRes = execPnpmSync(['ci'])
  expect(outdatedRes.status).not.toBe(0)
  expect(outdatedRes.stdout.toString() + outdatedRes.stderr.toString()).toContain('ERR_PNPM_OUTDATED_LOCKFILE')
  expect(fs.existsSync(markerPath)).toBe(true)
  expect(fs.existsSync(path.join('node_modules', 'is-positive'))).toBe(true)
})

test('pnpm ci with sharedWorkspaceLockfile=false preserves all node_modules when any project fails validation', () => {
  preparePackages([
    {
      name: 'pkg-a',
      version: '1.0.0',
      dependencies: { 'is-positive': '1.0.0' },
    },
    {
      name: 'pkg-b',
      version: '1.0.0',
      dependencies: { 'is-negative': '1.0.0' },
    },
  ])
  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['*'],
    sharedWorkspaceLockfile: false,
  })

  expect(execPnpmSync(['install']).status).toBe(0)
  const markerA = path.join('pkg-a', 'node_modules', 'marker-a.txt')
  const markerB = path.join('pkg-b', 'node_modules', 'marker-b.txt')
  fs.writeFileSync(markerA, 'a')
  fs.writeFileSync(markerB, 'b')

  fs.unlinkSync(path.join('pkg-b', 'pnpm-lock.yaml'))
  const ciRes = execPnpmSync(['ci'])
  expect(ciRes.status).not.toBe(0)
  expect(fs.existsSync(markerA)).toBe(true)
  expect(fs.existsSync(markerB)).toBe(true)
})

test('pnpm ci honors --filter and cleans root node_modules without root package.json when unfiltered', () => {
  preparePackages([
    {
      location: 'packages/pkg-a',
      package: {
        name: 'pkg-a',
        version: '1.0.0',
        dependencies: { 'is-positive': '1.0.0' },
      },
    },
    {
      location: 'packages/pkg-b',
      package: {
        name: 'pkg-b',
        version: '1.0.0',
        dependencies: { 'is-negative': '1.0.0' },
      },
    },
  ])
  if (fs.existsSync('package.json')) {
    fs.unlinkSync('package.json')
  }
  writeYamlFileSync('pnpm-workspace.yaml', { packages: ['packages/*'] })

  expect(execPnpmSync(['install']).status).toBe(0)

  const staleA = path.join('packages', 'pkg-a', 'node_modules', 'stale-a')
  const keepB = path.join('packages', 'pkg-b', 'node_modules', 'keep-b')
  const rootMarker = path.join('node_modules', 'root-stale-pkg')
  fs.mkdirSync(staleA, { recursive: true })
  fs.mkdirSync(keepB, { recursive: true })
  fs.mkdirSync(rootMarker, { recursive: true })

  // Filtered `pnpm --filter pkg-a ci` cleans and reinstalls only pkg-a
  const filteredRes = execPnpmSync(['--filter', 'pkg-a', 'ci'])
  expect(filteredRes.status).toBe(0)
  expect(fs.existsSync(staleA)).toBe(false)
  expect(fs.existsSync(path.join('packages', 'pkg-a', 'node_modules', 'is-positive'))).toBe(true)
  expect(fs.existsSync(keepB)).toBe(true)
  expect(fs.existsSync(rootMarker)).toBe(true)

  // Unfiltered `pnpm ci` cleans root node_modules even without root package.json
  const unfilteredRes = execPnpmSync(['ci'])
  expect(unfilteredRes.status).toBe(0)
  expect(fs.existsSync(rootMarker)).toBe(false)
  expect(fs.existsSync(keepB)).toBe(false)
  expect(fs.existsSync(path.join('packages', 'pkg-b', 'node_modules', 'is-negative'))).toBe(true)
})

test('pnpm ci preserves node_modules when pnprServer conflicts with frozenStore', () => {
  prepare({
    name: 'test-ci-pnpr-frozen-store',
    version: '1.0.0',
    dependencies: {
      'is-positive': '1.0.0',
    },
  })

  expect(execPnpmSync(['install']).status).toBe(0)
  const markerPath = path.join('node_modules', 'keep-marker.txt')
  fs.writeFileSync(markerPath, 'keep')

  const res = execPnpmSync(['ci', '--pnpr-server', 'https://pnpr.example.com', '--frozen-store'])
  expect(res.status).not.toBe(0)
  expect(res.stdout.toString() + res.stderr.toString()).toContain('ERR_PNPM_FROZEN_STORE_INCOMPATIBLE_WITH_PNPR')
  expect(fs.existsSync(markerPath)).toBe(true)
  expect(fs.existsSync(path.join('node_modules', 'is-positive'))).toBe(true)
})

test('pnpm ci cleans node_modules in a single project even when pnpm_config_filter is set in env', () => {
  prepare({
    name: 'single-app',
    version: '1.0.0',
    dependencies: {
      'is-positive': '1.0.0',
    },
  })

  expect(execPnpmSync(['install']).status).toBe(0)
  const stalePkg = path.join('node_modules', 'stale-pkg')
  fs.mkdirSync(stalePkg, { recursive: true })

  const res = execPnpmSync(['ci'], { env: { pnpm_config_filter: 'single-app' } })
  expect(res.status).toBe(0)
  expect(fs.existsSync(stalePkg)).toBe(false)
  expect(fs.existsSync(path.join('node_modules', 'is-positive'))).toBe(true)
})

test('pnpm -w ci preserves shared .pnpm virtual store for unselected workspace members', () => {
  preparePackages([
    {
      location: 'packages/pkg-a',
      package: {
        name: 'pkg-a',
        version: '1.0.0',
        dependencies: { 'is-negative': '1.0.0' },
      },
    },
  ])
  fs.writeFileSync('package.json', JSON.stringify({
    name: 'root-pkg',
    version: '1.0.0',
    private: true,
    dependencies: { 'is-positive': '1.0.0' },
  }))
  writeYamlFileSync('pnpm-workspace.yaml', { packages: ['packages/*'] })

  expect(execPnpmSync(['install']).status).toBe(0)
  fs.mkdirSync(path.join('node_modules', 'stale-root'), { recursive: true })

  const res = execPnpmSync(['-w', 'ci'])
  expect(res.status).toBe(0)
  expect(fs.existsSync(path.join('node_modules', 'stale-root'))).toBe(false)
  expect(fs.existsSync(path.join('node_modules', 'is-positive', 'package.json'))).toBe(true)
  expect(fs.existsSync(path.join('packages', 'pkg-a', 'node_modules', 'is-negative', 'package.json'))).toBe(true)
})

test('pnpm ci runs .pnpmfile.cjs hooks once and skips them when lockfile or checksum validation fails', () => {
  prepare({
    name: 'ci-hook-app',
    version: '1.0.0',
    dependencies: { 'is-positive': '1.0.0' },
  })
  const writePnpmfile = (tag: string): void => {
    fs.writeFileSync('.pnpmfile.cjs', [
      `// ${tag}`,
      "const fs = require('node:fs');",
      "const path = require('node:path');",
      "const logPath = path.join(__dirname, 'hooks.log');",
      'module.exports = {',
      '  hooks: {',
      '    readPackage(pkg) { fs.appendFileSync(logPath, `read:${pkg.name}\\n`); return pkg; },',
      "    preResolution() { fs.appendFileSync(logPath, 'preResolution\\n'); },",
      '  },',
      '};',
    ].join('\n'))
  }

  writePnpmfile('v1')
  expect(execPnpmSync(['install']).status).toBe(0)
  fs.unlinkSync('hooks.log')

  expect(execPnpmSync(['ci']).status).toBe(0)
  const lines = fs.readFileSync('hooks.log', 'utf8').trim().split('\n')
  expect(lines.filter((line) => line === 'read:ci-hook-app')).toHaveLength(1)
  expect(lines.filter((line) => line === 'preResolution')).toHaveLength(1)

  const marker = path.join('node_modules', 'keep-marker.txt')
  fs.writeFileSync(marker, 'keep')
  fs.unlinkSync('hooks.log')
  writePnpmfile('v2-drifted')

  const driftedRes = execPnpmSync(['ci'])
  expect(driftedRes.status).not.toBe(0)
  expect(driftedRes.stdout.toString() + driftedRes.stderr.toString()).toContain('ERR_PNPM_LOCKFILE_CONFIG_MISMATCH')
  expect(fs.existsSync('hooks.log')).toBe(false)
  expect(fs.existsSync(marker)).toBe(true)

  fs.unlinkSync('pnpm-lock.yaml')
  const missingRes = execPnpmSync(['ci'])
  expect(missingRes.status).not.toBe(0)
  expect(fs.existsSync('hooks.log')).toBe(false)
  expect(fs.existsSync(marker)).toBe(true)
})

test('pnpm ci rejects missing lockfile snapshot entry before cleaning node_modules', () => {
  prepare({
    name: 'ci-missing-snapshot',
    version: '1.0.0',
    dependencies: { 'is-positive': '1.0.0' },
  })
  expect(execPnpmSync(['install']).status).toBe(0)
  const marker = path.join('node_modules', 'keep-marker.txt')
  fs.writeFileSync(marker, 'keep')

  const rawLockfile = fs.readFileSync('pnpm-lock.yaml', 'utf8')
  const brokenLockfile = rawLockfile.replace(/snapshots:\n[\s\S]*$/, 'snapshots: {}\n')
  fs.writeFileSync('pnpm-lock.yaml', brokenLockfile)

  const res = execPnpmSync(['ci'])
  expect(res.status).not.toBe(0)
  expect(res.stdout.toString() + res.stderr.toString()).toContain('ERR_PNPM_LOCKFILE_MISSING_DEPENDENCY')
  expect(fs.existsSync(marker)).toBe(true)
})

test('pnpm ci cleans per-package modulesDir configured via packageConfigs', () => {
  preparePackages([
    {
      location: 'packages/pkg-a',
      package: {
        name: 'pkg-a',
        version: '1.0.0',
        dependencies: { 'is-positive': '1.0.0' },
      },
    },
  ])
  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['packages/*'],
    sharedWorkspaceLockfile: false,
    packageConfigs: {
      'pkg-a': {
        modulesDir: 'custom_nm',
      },
    },
  })

  expect(execPnpmSync(['install']).status).toBe(0)
  const staleDir = path.join('packages', 'pkg-a', 'custom_nm', 'stale-pkg')
  fs.mkdirSync(staleDir, { recursive: true })

  expect(execPnpmSync(['ci']).status).toBe(0)
  expect(fs.existsSync(staleDir)).toBe(false)
  expect(fs.existsSync(path.join('packages', 'pkg-a', 'custom_nm', 'is-positive', 'package.json'))).toBe(true)
})

test('pnpm ci validates missing lockfile with pnpmfile and runs preResolution once on conflict-merged lockfiles before cleaning', () => {
  prepare({
    name: 'ci-early-pre-resolution',
    version: '1.0.0',
  })
  const marker = path.join('node_modules', 'keep-marker.txt')
  fs.mkdirSync('node_modules', { recursive: true })
  fs.writeFileSync(marker, 'keep')

  // Local .pnpmfile.cjs without pnpm-lock.yaml fails checksum validation before cleaning or running hooks
  fs.writeFileSync('.pnpmfile.cjs', [
    'const fs = require("fs")',
    'module.exports = {',
    '  hooks: {',
    '    preResolution () {',
    '      fs.appendFileSync("hooks.log", "preResolution\\n")',
    '      if (process.env.FAIL_HOOK === "1") throw new Error("abort before clean")',
    '    },',
    '  },',
    '}',
  ].join('\n'))
  const mismatchRes = execPnpmSync(['ci'])
  expect(mismatchRes.status).not.toBe(0)
  expect(mismatchRes.stdout.toString() + mismatchRes.stderr.toString()).toContain('ERR_PNPM_LOCKFILE_CONFIG_MISMATCH')
  expect(fs.existsSync('hooks.log')).toBe(false)
  expect(fs.existsSync(marker)).toBe(true)

  // Conflict-marked lockfile (`lockfileHadConflicts`) runs preResolution before clean and reuses it once
  fs.writeFileSync('package.json', JSON.stringify({
    name: 'ci-early-pre-resolution',
    version: '1.0.0',
    dependencies: { 'is-positive': '1.0.0' },
  }))
  expect(execPnpmSync(['install']).status).toBe(0)
  fs.writeFileSync(marker, 'keep')
  const cleanLockfile = fs.readFileSync('pnpm-lock.yaml', 'utf8')
  const conflictedLockfile = `<<<<<<< HEAD\n${cleanLockfile}=======\n${cleanLockfile}>>>>>>> next\n`
  fs.writeFileSync('pnpm-lock.yaml', conflictedLockfile)
  if (fs.existsSync('hooks.log')) fs.unlinkSync('hooks.log')

  const failedRes = execPnpmSync(['ci'], { env: { FAIL_HOOK: '1' } })
  expect(failedRes.status).not.toBe(0)
  expect(fs.existsSync(marker)).toBe(true)

  fs.unlinkSync('hooks.log')
  const okRes = execPnpmSync(['ci'])
  expect(okRes.status).toBe(0)
  expect(fs.existsSync(marker)).toBe(false)
  const lines = fs.readFileSync('hooks.log', 'utf8').trim().split('\n')
  expect(lines).toStrictEqual(['preResolution'])
})
