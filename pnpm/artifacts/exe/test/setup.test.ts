import { execFileSync, spawnSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { describe, expect, test } from '@jest/globals'
import { familySync } from 'detect-libc'

// @ts-expect-error — JS helper without type declarations
import { exePlatformPkgName } from '../platform-pkg-name.js'

const exeDir = path.resolve(import.meta.dirname, '..')
const platform = process.platform
const isWindows = platform === 'win32'
// Match setup.js's detect-libc call so the fixture path lines up with the
// package `setup.js` actually resolves on this host (including musl).
const platformBin = path.join(
  exeDir, 'node_modules', exePlatformPkgName(platform, process.arch, familySync()),
  isWindows ? 'pnpm.exe' : 'pnpm'
)
const hasPlatformBinary = fs.existsSync(platformBin)
// dist/ is staged by the build-artifacts flow (not by `pn compile`), so
// ordinary test runs don't have it. The hardlink test is fine without it
// (existence + inode only), but the -v test actually executes the SEA, which
// loads dist/pnpm.mjs from next to the binary and would fail here.
const hasStagedBundle = fs.existsSync(path.join(exeDir, 'dist', 'pnpm.mjs'))

describe('exePlatformPkgName', () => {
  test('uses linuxstatic- prefix for linux + musl libc family', () => {
    expect(exePlatformPkgName('linux', 'x64', 'musl')).toBe('@pnpm/linuxstatic-x64')
    expect(exePlatformPkgName('linux', 'arm64', 'musl')).toBe('@pnpm/linuxstatic-arm64')
  })

  test('uses linux- prefix when libc is glibc or unknown', () => {
    expect(exePlatformPkgName('linux', 'x64', 'glibc')).toBe('@pnpm/linux-x64')
    expect(exePlatformPkgName('linux', 'arm64', null)).toBe('@pnpm/linux-arm64')
  })

  test('libc is irrelevant on non-linux platforms', () => {
    expect(exePlatformPkgName('darwin', 'arm64', 'musl')).toBe('@pnpm/macos-arm64')
    expect(exePlatformPkgName('darwin', 'x64', null)).toBe('@pnpm/macos-x64')
    expect(exePlatformPkgName('win32', 'x64', 'musl')).toBe('@pnpm/win-x64')
  })

  test('normalizes ia32 to x86 on win32 only', () => {
    expect(exePlatformPkgName('win32', 'ia32', null)).toBe('@pnpm/win-x86')
    expect(exePlatformPkgName('linux', 'ia32', null)).toBe('@pnpm/linux-ia32')
  })
})

test('prepare writes correct content for all bin files', () => {
  execFileSync(process.execPath, [path.join(exeDir, 'prepare.js')], { cwd: exeDir })

  // pnpm is a placeholder (replaced by setup.js with a hardlink)
  expect(fs.readFileSync(path.join(exeDir, 'pnpm'), 'utf8')).toBe('This file intentionally left blank')

  // pn, pnpx, and pnx should be real shell scripts
  for (const [name, command] of [['pn', 'pnpm'], ['pnpx', 'pnpm dlx'], ['pnx', 'pnpm dlx']]) {
    expect(fs.readFileSync(path.join(exeDir, name), 'utf8')).toBe(`#!/bin/sh\nexec ${command} "$@"\n`)
    if (!isWindows) {
      expect(fs.statSync(path.join(exeDir, name)).mode & 0o111).not.toBe(0)
    }
  }

  // Windows wrappers should exist
  for (const [name, command] of [['pn', 'pnpm'], ['pnpx', 'pnpm dlx'], ['pnx', 'pnpm dlx']]) {
    expect(fs.readFileSync(path.join(exeDir, name + '.cmd'), 'utf8')).toBe(`@echo off\n${command} %*\n`)
    expect(fs.readFileSync(path.join(exeDir, name + '.ps1'), 'utf8')).toBe(`${command} @args\n`)
  }
});

(hasPlatformBinary ? test : test.skip)('setup.js creates hardlink for pnpm', () => {
  execFileSync(process.execPath, [path.join(exeDir, 'prepare.js')], { cwd: exeDir })
  execFileSync(process.execPath, [path.join(exeDir, 'setup.js')], { cwd: exeDir })

  const pnpmBin = path.join(exeDir, isWindows ? 'pnpm.exe' : 'pnpm')
  expect(fs.statSync(pnpmBin).ino).toBe(fs.statSync(platformBin).ino)
});

// Actually execute the hardlinked pnpm binary. Existence and inode-match are
// not enough — a SEA blob built by a Node.js version that differs from the
// embedded runtime deserializes on startup with a native assertion and an
// abort signal, not a clean error exit (see rc.4 regression). Running `-v`
// verifies the SEA payload is actually readable by the embedded Node.
(hasPlatformBinary && hasStagedBundle ? test : test.skip)('pnpm -v runs and prints a semver', () => {
  execFileSync(process.execPath, [path.join(exeDir, 'prepare.js')], { cwd: exeDir })
  execFileSync(process.execPath, [path.join(exeDir, 'setup.js')], { cwd: exeDir })

  const pnpmBin = path.join(exeDir, isWindows ? 'pnpm.exe' : 'pnpm')
  const stdout = execFileSync(pnpmBin, ['-v'], { encoding: 'utf8', timeout: 30_000 }).trim()
  expect(stdout).toMatch(/^\d+\.\d+\.\d+(?:-[\w.-]+)?(?:\+[\w.-]+)?$/)
})

// Stand up a minimal sandbox that mimics @pnpm/exe with NO platform package
// installed: setup.js + platform-pkg-name.js + a package.json (so Node loads
// it as ESM), plus a node_modules with detect-libc symlinked from this repo
// so the script can reach the import.meta.resolve call we want to fail. The
// path-suffix of the fake exe dir controls whether the workspace skip fires.
function buildFailurePathSandbox (suffixSegments: string[]): string {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-exe-setup-test-'))
  const fakeExeDir = path.join(root, ...suffixSegments)
  fs.mkdirSync(fakeExeDir, { recursive: true })
  fs.copyFileSync(path.join(exeDir, 'setup.js'), path.join(fakeExeDir, 'setup.js'))
  fs.copyFileSync(path.join(exeDir, 'platform-pkg-name.js'), path.join(fakeExeDir, 'platform-pkg-name.js'))
  fs.writeFileSync(
    path.join(fakeExeDir, 'package.json'),
    JSON.stringify({ name: '@pnpm/exe', type: 'module' })
  )
  fs.mkdirSync(path.join(root, 'node_modules'))
  fs.symlinkSync(
    path.join(exeDir, 'node_modules', 'detect-libc'),
    path.join(root, 'node_modules', 'detect-libc'),
    'dir'
  )
  return fakeExeDir
}

// Skipping on Windows because fs.symlinkSync requires elevated privileges
// there for non-junction symlinks, and the path-suffix logic in setup.js is
// platform-independent — it's already exercised on Linux/macOS CI.
const failurePathTest = isWindows ? test.skip : test

failurePathTest('setup.js exits 0 silently when run from a workspace-shaped path with no platform package', () => {
  const fakeExeDir = buildFailurePathSandbox(['pnpm', 'artifacts', 'exe'])
  const result = spawnSync(process.execPath, [path.join(fakeExeDir, 'setup.js')], {
    encoding: 'utf8',
    timeout: 10_000,
  })
  expect({ status: result.status, stderr: result.stderr, stdout: result.stdout })
    .toEqual({ status: 0, stderr: '', stdout: '' })
})

failurePathTest('setup.js exits 1 with the missing platform package name when run from a non-workspace path', () => {
  const fakeExeDir = buildFailurePathSandbox(['somewhere', 'else'])
  const result = spawnSync(process.execPath, [path.join(fakeExeDir, 'setup.js')], {
    encoding: 'utf8',
    timeout: 10_000,
  })
  const expectedPkgName = exePlatformPkgName(platform, process.arch, familySync())
  expect(result.status).toBe(1)
  // On darwin-x64 the message is the dedicated Intel-Mac one (mentions the
  // upstream Node.js issue); on every other host it's the generic one that
  // names the missing platform package. Both reference the package name, so
  // assert on that.
  expect(result.stderr).toContain(expectedPkgName === '@pnpm/macos-x64' ? '11423' : expectedPkgName)
})
