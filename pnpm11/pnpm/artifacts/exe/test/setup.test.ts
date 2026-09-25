import { execFileSync, spawnSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { describe, expect, test } from '@jest/globals'
import { cmdShim } from '@pnpm/bins.cmd-shim'
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
// Each alias bin, the text prepare.js appends to its pnpm call, and the argv
// that text produces: `pnpx` and `pnx` are `pnpm dlx`.
const ALIASES = [
  { name: 'pn', shell: '', argv: [] as string[] },
  { name: 'pnpx', shell: ' dlx', argv: ['dlx'] },
  { name: 'pnx', shell: ' dlx', argv: ['dlx'] },
]

// Read before the tests below run prepare.js in this directory, which overwrites
// the committed copies with whatever the generator produces now.
const COMMITTED_ALIASES = new Map(ALIASES.map(({ name }) => [name, fs.readFileSync(path.join(exeDir, name), 'utf8')]))

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

  // pn, pnpx, and pnx should be real shell scripts that hand over to the pnpm
  // beside them; what they do with it is covered by 'alias bins' below.
  for (const { name, shell } of ALIASES) {
    const script = fs.readFileSync(path.join(exeDir, name), 'utf8')
    expect(script.startsWith('#!/bin/sh\n')).toBe(true)
    expect(script).toContain(`exec "$pnpm"${shell} "$@"\n`)
    if (!isWindows) {
      expect(fs.statSync(path.join(exeDir, name)).mode & 0o111).not.toBe(0)
    }
  }

  // Windows wrappers should exist. setup.js hardlinks the binary onto
  // pn.exe/pnpx.exe/pnx.exe and points `bin` at those, so these only run when
  // setup.js did not — where there is no sibling binary and PATH is all they have.
  for (const { name, shell } of ALIASES) {
    expect(fs.readFileSync(path.join(exeDir, name + '.cmd'), 'utf8')).toBe(`@echo off\npnpm${shell} %*\nexit /b %errorlevel%\n`)
    expect(fs.readFileSync(path.join(exeDir, name + '.ps1'), 'utf8')).toBe(`pnpm${shell} @args\nexit $LASTEXITCODE\n`)
  }
});

// prepare.js rewrites the alias scripts on every install, so an edit made to a
// committed copy alone lasts only until the next one.
test('the committed alias scripts are what prepare.js writes', () => {
  const sandbox = buildAliasSandbox()

  for (const { name } of ALIASES) {
    const generated = fs.readFileSync(path.join(sandbox, name), 'utf8')
    expect({ name, script: COMMITTED_ALIASES.get(name) }).toEqual({ name, script: generated })
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

const npmShimTest = isWindows ? test : test.skip

npmShimTest('npm global PowerShell shim waits for the standalone executable', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-exe-npm-shim-'))
  try {
    const prefix = installExeFixtureWithNpm(root, ['--global'])
    expectShimsRunTheExecutable(prefix)
  } finally {
    fs.rmSync(root, { recursive: true, force: true })
  }
})

// `--location=global` leaves npm_config_global unset and sets npm's project
// prefix to the global prefix, so only npm_config_location marks it global.
npmShimTest('npm global shims name the standalone executable with --location=global', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-exe-npm-shim-'))
  try {
    const prefix = installExeFixtureWithNpm(root, ['--location=global'])
    expectShimsRunTheExecutable(prefix)
    expect(fs.existsSync(path.join(prefix, 'node_modules', '.bin'))).toBe(false)
  } finally {
    fs.rmSync(root, { recursive: true, force: true })
  }
})

npmShimTest('npm project shims name the standalone executable', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-exe-npm-shim-'))
  try {
    const prefix = installExeFixtureWithNpm(root, [])
    expectShimsRunTheExecutable(path.join(prefix, 'node_modules', '.bin'))
  } finally {
    fs.rmSync(root, { recursive: true, force: true })
  }
})

/**
 * Install a minimal @pnpm/exe, whose platform package carries node.exe as the
 * standalone executable, with npm into `<root>/prefix`.
 */
function installExeFixtureWithNpm (root: string, npmFlags: string[]): string {
  const nativePackageDir = path.join(root, 'native-package')
  fs.mkdirSync(nativePackageDir)
  const nativePackageName = exePlatformPkgName(platform, process.arch, familySync())
  fs.writeFileSync(path.join(nativePackageDir, 'package.json'), JSON.stringify({
    name: nativePackageName,
    version: '1.0.0',
  }))
  const nativeBinary = path.join(nativePackageDir, 'pnpm.exe')
  try {
    fs.linkSync(process.execPath, nativeBinary)
  } catch (err) {
    if ((err as NodeJS.ErrnoException).code !== 'EXDEV') throw err
    fs.copyFileSync(process.execPath, nativeBinary)
  }

  const fixtureDir = path.join(root, 'wrapper')
  fs.mkdirSync(fixtureDir)
  for (const name of ['setup.js', 'platform-pkg-name.js']) {
    fs.copyFileSync(path.join(exeDir, name), path.join(fixtureDir, name))
  }
  for (const name of ['pnpm', 'pn', 'pnpx', 'pnx']) {
    fs.writeFileSync(path.join(fixtureDir, name), 'placeholder')
  }
  const exeManifest = JSON.parse(fs.readFileSync(path.join(exeDir, 'package.json'), 'utf8')) as {
    scripts: { preinstall: string, postinstall?: string },
  }
  fs.writeFileSync(path.join(fixtureDir, 'package.json'), JSON.stringify({
    name: '@pnpm/exe',
    version: '1.0.0',
    type: 'module',
    bin: { pnpm: 'pnpm', pn: 'pn', pnpx: 'pnpx', pnx: 'pnx' },
    scripts: {
      preinstall: exeManifest.scripts.preinstall,
      postinstall: exeManifest.scripts.postinstall,
    },
    dependencies: {
      'detect-libc': `file:${fs.realpathSync(path.join(exeDir, 'node_modules', 'detect-libc'))}`,
    },
    optionalDependencies: { [nativePackageName]: `file:${nativePackageDir}` },
  }))

  const npmCli = execFileSync('where.exe', ['npm.cmd'], { encoding: 'utf8' })
    .split(/\r?\n/)
    .filter(Boolean)
    .map(launcher => path.join(path.dirname(launcher), 'node_modules', 'npm', 'bin', 'npm-cli.js'))
    .find(candidate => fs.existsSync(candidate))
  expect(npmCli).toBeDefined()
  const prefix = path.join(root, 'prefix')
  fs.mkdirSync(prefix)
  if (!npmFlags.some(flag => flag.startsWith('--global') || flag.startsWith('--location'))) {
    fs.writeFileSync(path.join(prefix, 'package.json'), JSON.stringify({ name: 'project', version: '1.0.0' }))
  }
  execFileSync(process.execPath, [
    npmCli!, 'install', ...npmFlags, '--install-links=true', '--dangerously-allow-all-scripts',
    '--prefix', prefix, fixtureDir,
  ], { cwd: root, stdio: 'pipe', timeout: 60_000 })
  return prefix
}

function expectShimsRunTheExecutable (binDir: string): void {
  for (const name of ['pnpm', 'pn', 'pnpx', 'pnx']) {
    for (const ext of ['cmd', 'ps1']) {
      expect(fs.readFileSync(path.join(binDir, `${name}.${ext}`), 'utf8')).toContain(`${name}.exe`)
    }
  }
  const shim = path.join(binDir, 'pnpm.ps1')
  expect(execFileSync('powershell.exe', ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', shim, '--version'], {
    encoding: 'utf8',
    timeout: 10_000,
  }).trim()).toBe(process.version)
  const failure = spawnSync('powershell.exe', [
    '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', shim, '-e', 'process.exit(7)',
  ], { encoding: 'utf8', timeout: 10_000 })
  expect(failure.status).toBe(7)
}

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
  // Seed the platform binary with the test's own node.exe. setup.js then
  // hardlinks it again into the sandbox @pnpm/exe dir; downstream tests can
  // invoke the resulting `pnpx.exe` (etc.) and assert the alias actually ran.
  // A hardlink is enough and avoids copying ~80MB, but it can't span volumes
  // — on GitHub's Windows runners the workspace (and node) sit on `D:` while
  // `os.tmpdir()` is on `C:`, so fall back to a copy when the link is
  // cross-device. The seed's identity doesn't matter here; the assertions
  // exercise setup.js's own intra-sandbox hardlinking.
  const seedTarget = path.join(platformDir, 'pnpm.exe')
  try {
    fs.linkSync(process.execPath, seedTarget)
  } catch (err) {
    if ((err as NodeJS.ErrnoException).code !== 'EXDEV') throw err
    fs.copyFileSync(process.execPath, seedTarget)
  }
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
  // entry into @pnpm/bins.cmd-shim and let it write the Bash / cmd / pwsh
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

// A PATH with no pnpm on it, so a lookup there finds nothing but the decoys the
// tests plant. `readlink` and `dirname` still have to be reachable.
const BARE_PATH = '/usr/bin:/bin'
// The alias scripts are `sh` scripts, and Windows has no `sh`. It never runs
// them anyway: setup.js replaces them with hardlinks of the native binary, which
// the winSetupTest above covers.
const aliasTest = isWindows ? test.skip : test

describe('alias bins', () => {
  for (const { name, argv } of ALIASES) {
    const expected = `sibling: ${[...argv, 'add', 'foo'].join(' ')}\n`

    // The native binary sits next to the alias, so it is reachable even where
    // the directory holding both is not on PATH — as node_modules/.bin is not,
    // outside a `pnpm run`.
    aliasTest(`${name} runs the pnpm beside it with no pnpm on PATH`, () => {
      const sandbox = buildAliasSandbox()

      const result = runAlias(path.join(sandbox, name), BARE_PATH)
      expect({ status: result.status, stdout: result.stdout, stderr: result.stderr })
        .toEqual({ status: 0, stdout: expected, stderr: '' })
    })

    // Any other pnpm on PATH — a different major installed globally, or a
    // wrapper of one — would otherwise take over the call, and say nothing.
    aliasTest(`${name} ignores an unrelated pnpm earlier on PATH`, () => {
      const sandbox = buildAliasSandbox()
      const decoyDir = path.join(sandbox, 'decoy')
      writeStub(path.join(decoyDir, 'pnpm'), 'decoy')

      const result = runAlias(path.join(sandbox, name), `${decoyDir}:${BARE_PATH}`)
      expect({ status: result.status, stdout: result.stdout }).toEqual({ status: 0, stdout: expected })
    })

    // A bin directory links the alias from this package while its own pnpm comes
    // from elsewhere, or is missing. The alias belongs to the package it was
    // linked from, so that is the pnpm it has to reach.
    aliasTest(`${name} resolves past a symlink to the package it was linked from`, () => {
      const sandbox = buildAliasSandbox()
      const binDir = path.join(sandbox, 'global-bin')
      writeStub(path.join(binDir, 'pnpm'), 'decoy')
      fs.symlinkSync(path.join(sandbox, name), path.join(binDir, name))

      const result = runAlias(path.join(binDir, name), BARE_PATH)
      expect({ status: result.status, stdout: result.stdout }).toEqual({ status: 0, stdout: expected })
    })

    // pnpm/pnpm#14884: MSYS and Cygwin launch the alias with a native Windows
    // path, which has no slash for ${self%/*} to strip. Only a drive letter or a
    // UNC prefix marks one, since a backslash is an ordinary character in a Unix
    // file name. Each test below plants the alias and its sibling pnpm where the
    // path resolves to, and hands sh the file whose own name is that path.
    aliasTest(`${name} resolves a drive-letter $0`, () => {
      const sandbox = buildAliasSandbox()
      const arg0 = `C:\\proj\\${name}`
      fs.copyFileSync(plantAliasAndPnpm(sandbox, name, path.join(sandbox, 'C:', 'proj')), path.join(sandbox, arg0))

      const result = runNativeAlias(sandbox, arg0)
      expect({ status: result.status, stdout: result.stdout }).toEqual({ status: 0, stdout: expected })
    })

    // A UNC path converts to one starting with //, which Linux and macOS read as
    // /, so the share stands in for the sandbox directory itself. `shareDir` is
    // absolute, so its own leading separator is the second of the two backslashes
    // that mark the path as UNC.
    aliasTest(`${name} resolves a UNC $0`, () => {
      const sandbox = buildAliasSandbox()
      const shareDir = path.join(sandbox, 'share')
      const arg0 = `\\${shareDir}/${name}`.replaceAll('/', '\\')
      fs.copyFileSync(plantAliasAndPnpm(sandbox, name, shareDir), path.join(sandbox, arg0))

      const result = runNativeAlias(sandbox, arg0)
      expect({ status: result.status, stdout: result.stdout }).toEqual({ status: 0, stdout: expected })
    })

    // The other side of the gate: neither prefix is there, so the backslash stays
    // part of the directory name rather than becoming a separator.
    aliasTest(`${name} leaves a Unix path holding a backslash alone`, () => {
      const sandbox = buildAliasSandbox()

      const result = runAlias(plantAliasAndPnpm(sandbox, name, path.join(sandbox, 'proj\\dir')), BARE_PATH)
      expect({ status: result.status, stdout: result.stdout }).toEqual({ status: 0, stdout: expected })
    })

    aliasTest(`${name} reports a skipped install script rather than failing to exec the placeholder`, () => {
      const sandbox = buildAliasSandbox({ installBinary: false })

      const result = runAlias(path.join(sandbox, name), BARE_PATH)
      expect(result.status).toBe(1)
      expect(result.stderr).toContain(`${name}: pnpm's native binary was not installed next to this script.`)
    })
  }
})

const SYSTEM32 = path.join(process.env.SystemRoot ?? 'C:\\Windows', 'System32')
const POWERSHELL_DIR = path.join(SYSTEM32, 'WindowsPowerShell', 'v1.0')

const winCmdTest = isWindows ? test : test.skip

const powershellCommand = (() => {
  if (isWindows) {
    const probe = spawnSync('powershell', ['-NoProfile', '-Command', 'exit 0'], {
      encoding: 'utf8',
      timeout: 5_000,
      env: { ...process.env, PATH: `${POWERSHELL_DIR};${SYSTEM32};${process.env.PATH ?? ''}` },
    })
    if (probe.status === 0) return 'powershell'
    const probePwsh = spawnSync('pwsh', ['-NoProfile', '-Command', 'exit 0'], { encoding: 'utf8', timeout: 5_000 })
    if (probePwsh.status === 0) return 'pwsh'
    return null
  }
  const probe = spawnSync('pwsh', ['-NoProfile', '-NonInteractive', '-Command', 'exit 0'], { encoding: 'utf8', timeout: 5_000 })
  return probe.status === 0 ? 'pwsh' : null
})()
const powershellTest = powershellCommand != null ? test : test.skip

describe('Windows fallback wrappers', () => {
  for (const { name, argv } of ALIASES) {
    const expected = `stub: ${[...argv, 'add', 'foo'].join(' ')}`

    winCmdTest(`${name}.cmd propagates non-zero exit status from pnpm on PATH`, () => {
      const { sandbox, stubDir } = buildFallbackSandbox()
      try {
        const result = spawnSync('cmd.exe', ['/d', '/c', path.join(sandbox, `${name}.cmd`), 'fail'], {
          cwd: sandbox,
          encoding: 'utf8',
          timeout: 10_000,
          env: getWindowsFallbackEnv(stubDir),
        })
        expect({ status: result.status, stderr: result.stderr }).toEqual({
          status: 42,
          stderr: '',
        })
      } finally {
        fs.rmSync(sandbox, { recursive: true, force: true })
      }
    })

    winCmdTest(`${name}.cmd propagates successful exit status and arguments`, () => {
      const { sandbox, stubDir } = buildFallbackSandbox()
      try {
        const result = spawnSync('cmd.exe', ['/d', '/c', path.join(sandbox, `${name}.cmd`), 'add', 'foo'], {
          cwd: sandbox,
          encoding: 'utf8',
          timeout: 10_000,
          env: getWindowsFallbackEnv(stubDir),
        })
        expect({ status: result.status, stdout: result.stdout.trim(), stderr: result.stderr }).toEqual({
          status: 0,
          stdout: expected,
          stderr: '',
        })
      } finally {
        fs.rmSync(sandbox, { recursive: true, force: true })
      }
    })

    powershellTest(`${name}.ps1 propagates non-zero exit status from pnpm on PATH`, () => {
      const { sandbox, stubDir } = buildFallbackSandbox()
      try {
        const result = spawnSync(powershellCommand!, ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', path.join(sandbox, `${name}.ps1`), 'fail'], {
          cwd: sandbox,
          encoding: 'utf8',
          timeout: 10_000,
          env: getWindowsFallbackEnv(stubDir),
        })
        expect({ status: result.status, stderr: result.stderr }).toEqual({
          status: 42,
          stderr: '',
        })
      } finally {
        fs.rmSync(sandbox, { recursive: true, force: true })
      }
    })

    powershellTest(`${name}.ps1 propagates successful exit status and arguments`, () => {
      const { sandbox, stubDir } = buildFallbackSandbox()
      try {
        const result = spawnSync(powershellCommand!, ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', path.join(sandbox, `${name}.ps1`), 'add', 'foo'], {
          cwd: sandbox,
          encoding: 'utf8',
          timeout: 10_000,
          env: getWindowsFallbackEnv(stubDir),
        })
        expect({ status: result.status, stdout: result.stdout.trim(), stderr: result.stderr }).toEqual({
          status: 0,
          stdout: expected,
          stderr: '',
        })
      } finally {
        fs.rmSync(sandbox, { recursive: true, force: true })
      }
    })
  }
})

/**
 * An @pnpm/exe directory as prepare.js leaves it, with `pnpm` replaced by a
 * stand-in that reports the arguments it was handed — which is all the aliases
 * have to get right, and what setup.js's hardlink of the native binary occupies.
 * `installBinary: false` leaves prepare.js's placeholder there instead, standing
 * for an install whose scripts were skipped.
 */
function buildAliasSandbox ({ installBinary = true } = {}): string {
  const sandbox = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-exe-alias-'))
  fs.copyFileSync(path.join(exeDir, 'prepare.js'), path.join(sandbox, 'prepare.js'))
  fs.writeFileSync(path.join(sandbox, 'package.json'), JSON.stringify({ name: '@pnpm/exe', type: 'module' }))
  execFileSync(process.execPath, [path.join(sandbox, 'prepare.js')], { cwd: sandbox })
  if (installBinary) {
    writeStub(path.join(sandbox, 'pnpm'), 'sibling')
  }
  return sandbox
}

function plantAliasAndPnpm (sandbox: string, name: string, targetDir: string): string {
  fs.mkdirSync(targetDir, { recursive: true })
  const file = path.join(targetDir, name)
  fs.copyFileSync(path.join(sandbox, name), file)
  fs.chmodSync(file, 0o755)
  writeStub(path.join(targetDir, 'pnpm'), 'sibling')
  return file
}

/**
 * An executable stand-in for pnpm at `file` that echoes `label` and its
 * arguments. chmod separately: `writeFileSync`'s `mode` applies only when it
 * creates the file, and here it overwrites prepare.js's non-executable placeholder.
 */
function writeStub (file: string, label: string): void {
  fs.mkdirSync(path.dirname(file), { recursive: true })
  fs.writeFileSync(file, `#!/bin/sh\necho "${label}: $*"\n`)
  fs.chmodSync(file, 0o755)
}

function runAlias (alias: string, pathEnv: string) {
  return spawnSync(alias, ['add', 'foo'], {
    encoding: 'utf8',
    timeout: 10_000,
    env: { ...process.env, PATH: pathEnv },
  })
}

function runNativeAlias (cwd: string, arg0: string) {
  return spawnSync('sh', [arg0, 'add', 'foo'], {
    cwd,
    encoding: 'utf8',
    timeout: 10_000,
    env: { ...process.env, PATH: BARE_PATH },
  })
}

function buildFallbackSandbox (): { sandbox: string, stubDir: string } {
  const sandbox = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-exe-fallback-'))
  fs.copyFileSync(path.join(exeDir, 'prepare.js'), path.join(sandbox, 'prepare.js'))
  fs.writeFileSync(path.join(sandbox, 'package.json'), JSON.stringify({ name: '@pnpm/exe', type: 'module' }))
  execFileSync(process.execPath, [path.join(sandbox, 'prepare.js')], { cwd: sandbox })

  const stubDir = path.join(sandbox, 'stub')
  fs.mkdirSync(stubDir, { recursive: true })

  // Use an executable binary stand-in (node) for pnpm, so on Windows cmd.exe executes
  // an .exe via CreateProcess and regains control in the wrapper rather than transferring
  // execution to a chained .cmd batch file.
  const stubJs = path.join(stubDir, 'stub.cjs')
  fs.writeFileSync(
    stubJs,
    `const path = require('path')
const args = process.argv.slice(1).map((arg) => {
  const base = path.basename(arg)
  return ['dlx', 'fail', 'add', 'foo'].includes(base) ? base : arg
})
if (args.includes('fail')) {
  process.exit(42)
}
console.log('stub: ' + args.join(' '))
process.exit(0)
`
  )

  const pnpmBin = path.join(stubDir, isWindows ? 'pnpm.exe' : 'pnpm')
  try {
    fs.linkSync(process.execPath, pnpmBin)
  } catch (err) {
    if ((err as NodeJS.ErrnoException).code !== 'EXDEV') throw err
    fs.copyFileSync(process.execPath, pnpmBin)
  }

  return { sandbox, stubDir }
}

function getWindowsFallbackEnv (stubDir: string): NodeJS.ProcessEnv {
  const pathParts = isWindows
    ? [stubDir, POWERSHELL_DIR, SYSTEM32, process.env.PATH]
    : [stubDir, process.env.PATH]
  const stubJs = path.join(stubDir, 'stub.cjs').replace(/\\/g, '/')
  const prevNodeOptions = process.env.NODE_OPTIONS ?? ''
  return {
    ...process.env,
    PATH: pathParts.filter(Boolean).join(path.delimiter),
    NODE_OPTIONS: `${prevNodeOptions} --require "${stubJs}"`.trim(),
  }
}
