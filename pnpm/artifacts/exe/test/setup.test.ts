import { execFileSync, spawnSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { describe, expect, test } from '@jest/globals'
import { cmdShim } from '@zkochan/cmd-shim'
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

// Build a sandboxed @pnpm/exe install with a real .exe playing the part of
// pnpm.exe (we use the running node binary — setup.js only hardlinks it) and
// run setup.js. Returns the sandbox directory.
function buildWinSetupSandbox (): string {
  const sandbox = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-exe-fix11486-'))
  fs.copyFileSync(path.join(exeDir, 'setup.js'), path.join(sandbox, 'setup.js'))
  fs.copyFileSync(path.join(exeDir, 'prepare.js'), path.join(sandbox, 'prepare.js'))
  fs.copyFileSync(path.join(exeDir, 'platform-pkg-name.js'), path.join(sandbox, 'platform-pkg-name.js'))
  fs.writeFileSync(path.join(sandbox, 'package.json'), JSON.stringify({
    name: '@pnpm/exe',
    type: 'module',
    bin: { pnpm: 'pnpm', pn: 'pn', pnpx: 'pnpx', pnx: 'pnx' },
  }))

  const platformPkgName = exePlatformPkgName(platform, process.arch, familySync())
  const platformDir = path.join(sandbox, 'node_modules', platformPkgName)
  fs.mkdirSync(platformDir, { recursive: true })
  fs.writeFileSync(path.join(platformDir, 'package.json'), JSON.stringify({
    name: platformPkgName, version: '0.0.0',
  }))
  // Hardlink the test's own node.exe as the platform binary. setup.js then
  // hardlinks it again into the sandbox @pnpm/exe dir; downstream tests can
  // invoke the resulting `pnpx.exe` (etc.) and assert the alias actually ran.
  fs.linkSync(process.execPath, path.join(platformDir, 'pnpm.exe'))
  // platform-pkg-name.js calls into detect-libc; make the package resolvable
  // from the sandbox. On Windows, use a junction — non-junction directory
  // symlinks require Developer Mode or admin privileges, which Windows CI and
  // most local Windows dev setups don't have. (See the failure-path tests
  // higher in this file: they skip on Windows for the same reason.)
  fs.symlinkSync(
    path.join(exeDir, 'node_modules', 'detect-libc'),
    path.join(sandbox, 'node_modules', 'detect-libc'),
    isWindows ? 'junction' : 'dir'
  )

  execFileSync(process.execPath, [path.join(sandbox, 'prepare.js')], { cwd: sandbox })
  execFileSync(process.execPath, [path.join(sandbox, 'setup.js')], { cwd: sandbox })

  return sandbox
}

const winSetupTest = isWindows ? test : test.skip

// Regression coverage for https://github.com/pnpm/pnpm/issues/11486.
// See the matching describe block in
// engine/pm/commands/test/self-updater/selfUpdate.test.ts for the full
// rationale; this one covers the @pnpm/exe preinstall path that handles
// fresh `npm install -g @pnpm/exe`.
winSetupTest('setup.js (Windows) rewrites bin to .exe entries and hardlinks pn/pnpx/pnx aliases (issue #11486)', () => {
  const sandbox = buildWinSetupSandbox()
  const pkg = JSON.parse(fs.readFileSync(path.join(sandbox, 'package.json'), 'utf8'))

  expect(pkg.bin).toEqual({
    pnpm: 'pnpm.exe',
    pn: 'pn.exe',
    pnpx: 'pnpx.exe',
    pnx: 'pnx.exe',
  })

  const pnpmIno = fs.statSync(path.join(sandbox, 'pnpm.exe')).ino
  for (const name of ['pn', 'pnpx', 'pnx']) {
    const aliasPath = path.join(sandbox, `${name}.exe`)
    expect(fs.existsSync(aliasPath)).toBe(true)
    expect(fs.statSync(aliasPath).ino).toBe(pnpmIno)
  }
})

// The Bash-shim end-to-end repro depends on Git Bash / MSYS2. CI runners
// (windows-latest) ship Git Bash on PATH, but local Windows dev machines
// often don't, so probe before running and skip the test cleanly otherwise
// (rather than spawning bash and getting an opaque ENOENT).
const bashAvailable = (() => {
  if (!isWindows) return false
  const probe = spawnSync('bash', ['--version'], { encoding: 'utf8', timeout: 5_000 })
  return probe.status === 0
})()
const winBashTest = bashAvailable ? test : test.skip

winBashTest('aliases run from Bash (Git Bash / MSYS2) without dropping into interactive cmd.exe (issue #11486)', async () => {
  const sandbox = buildWinSetupSandbox()
  const pkg = JSON.parse(fs.readFileSync(path.join(sandbox, 'package.json'), 'utf8'))

  // Mirror what `pnpm self-update` does in the global bin: feed each bin
  // entry into @zkochan/cmd-shim and let it write the Bash / cmd / pwsh
  // shims. Using cmd-shim here (the same lib pnpm's bin linker uses) is what
  // lets this repro the real-world chain rather than just asserting the
  // package.json shape.
  const binDir = path.join(sandbox, 'global-bin')
  await Promise.all(Object.entries(pkg.bin).map(([name, target]) =>
    cmdShim(path.join(sandbox, target as string), path.join(binDir, name), { createPwshFile: true })
  ))

  for (const alias of ['pn', 'pnpx', 'pnx']) {
    const shim = path.join(binDir, alias).replace(/\\/g, '/')
    // The shim's target is hardlinked to node.exe in this test (it's the
    // SEA pnpm.exe in production), so `-e "..."` lets us assert the alias
    // really ran our snippet — a successful assertion implies the cmd.exe
    // hop got bypassed.
    const result = spawnSync('bash', ['-c', `'${shim}' -e "process.stdout.write('${alias}_OK')"`], {
      encoding: 'utf8',
      timeout: 30_000,
    })

    // Pre-fix symptom: cmd-shim's Bash shim for a .cmd target does
    // `exec cmd /C ...`. MSYS2 mangles `/C` into a Windows path before
    // cmd.exe sees it; cmd.exe finds no /C or /K and falls into interactive
    // mode, printing its banner instead of running the alias.
    expect({
      alias,
      status: result.status,
      banner: /Microsoft Windows/.test(result.stdout + result.stderr),
      stdout: result.stdout,
    }).toEqual({
      alias,
      status: 0,
      banner: false,
      stdout: `${alias}_OK`,
    })
  }
})
