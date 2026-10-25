import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { describe, test } from 'node:test'

import { detectInstallOrigin, findShadowingPnpm, pnpmHomeDir, renderShadowingPnpmWarning } from '../lib/shadowingPnpm.js'

// The file name a PATH lookup for `pnpm` accepts on this platform.
const PNPM = process.platform === 'win32' ? 'pnpm.cmd' : 'pnpm'

function tempRoot (): string {
  return fs.mkdtempSync(path.join(os.tmpdir(), 'get-pnpm-shadowing-'))
}

function writeExecutable (dir: string, name: string, contents = ''): string {
  fs.mkdirSync(dir, { recursive: true })
  const file = path.join(dir, name)
  fs.writeFileSync(file, contents, { mode: 0o755 })
  return file
}

function pathEnv (dirs: string[]): string {
  return dirs.join(path.delimiter)
}

describe('findShadowingPnpm', () => {
  test('nothing shadows without a PATH', () => {
    assert.equal(findShadowingPnpm(tempRoot(), { pathEnv: undefined }), undefined)
  })

  test('nothing shadows when the pnpm home bin comes first', () => {
    const home = tempRoot()
    const bin = path.join(home, 'bin')
    writeExecutable(bin, PNPM)
    const other = path.join(tempRoot(), 'other')
    writeExecutable(other, PNPM)

    assert.equal(findShadowingPnpm(home, { pathEnv: pathEnv([bin, other]) }), undefined)
  })

  test('nothing shadows when no pnpm is on the PATH', () => {
    const empty = path.join(tempRoot(), 'empty')
    fs.mkdirSync(empty)

    assert.equal(findShadowingPnpm(tempRoot(), { pathEnv: pathEnv([empty]) }), undefined)
  })

  test('a pnpm ahead of the pnpm home bin shadows it', () => {
    const home = tempRoot()
    const bin = path.join(home, 'bin')
    writeExecutable(bin, PNPM)
    const other = path.join(tempRoot(), 'other')
    const executable = writeExecutable(other, PNPM)

    assert.deepEqual(findShadowingPnpm(home, { pathEnv: pathEnv([other, bin]) }), {
      executable,
      origin: 'unknown',
      binOnPath: true,
    })
  })

  test('a pnpm shadows a pnpm home bin that is not on the PATH yet', () => {
    const other = path.join(tempRoot(), 'other')
    const executable = writeExecutable(other, PNPM)

    assert.deepEqual(findShadowingPnpm(tempRoot(), { pathEnv: pathEnv([other]) }), {
      executable,
      origin: 'unknown',
      binOnPath: false,
    })
  })

  // The v10 layout links `pnpm` straight into the pnpm home directory; that is
  // pnpm's own, not another installer's.
  test('a pnpm in the pnpm home directory is pnpm itself', () => {
    const home = tempRoot()
    writeExecutable(home, PNPM)

    assert.equal(findShadowingPnpm(home, { pathEnv: pathEnv([home]) }), undefined)
  })

  test('a symlink into the pnpm home bin is pnpm itself', () => {
    const home = tempRoot()
    const bin = path.join(home, 'bin')
    const executable = writeExecutable(bin, PNPM)
    const links = path.join(tempRoot(), 'links')
    fs.mkdirSync(links)
    fs.symlinkSync(executable, path.join(links, PNPM))

    assert.equal(findShadowingPnpm(home, { pathEnv: pathEnv([links, bin]) }), undefined)
  })

  test('looks up every PATHEXT extension on Windows', () => {
    const other = path.join(tempRoot(), 'other')
    const executable = writeExecutable(other, 'pnpm.CMD')

    assert.equal(
      findShadowingPnpm(tempRoot(), { pathEnv: other, platform: 'win32', pathExt: '.EXE;.CMD' })?.executable,
      path.join(other, 'pnpm.CMD')
    )
    assert.equal(fs.existsSync(executable), true)
  })

  test('names a Windows executable as it is spelled on disk, not in PATHEXT', () => {
    const other = path.join(tempRoot(), 'other')
    const executable = writeExecutable(other, 'pnpm.cmd')

    assert.equal(
      findShadowingPnpm(tempRoot(), { pathEnv: other, platform: 'win32', pathExt: '.EXE;.CMD' })?.executable,
      executable
    )
  })
})

describe('detectInstallOrigin', () => {
  for (const [relative, expected] of [
    ['opt/homebrew/lib/node_modules/pnpm/bin/pnpm.mjs', 'npm'],
    ['opt/homebrew/Cellar/pnpm/12.6.0/bin/pnpm', 'homebrew'],
    ['usr/local/lib/node_modules/corepack/shims/pnpm', 'corepack'],
    ['home/me/.volta/bin/pnpm', 'volta'],
    ['Users/me/scoop/shims/pnpm.cmd', 'scoop'],
    ['home/me/bin/pnpm', 'unknown'],
  ]) {
    test(`is read off the path ${relative}`, () => {
      assert.equal(detectInstallOrigin(path.join(tempRoot(), relative)), expected)
    })
  }

  test('follows a symlink to its target', () => {
    const root = tempRoot()
    const target = writeExecutable(path.join(root, 'lib', 'node_modules', 'pnpm', 'bin'), 'pnpm.mjs')
    const link = path.join(root, 'bin', 'pnpm')
    fs.mkdirSync(path.dirname(link))
    fs.symlinkSync(target, link)

    assert.equal(detectInstallOrigin(link), 'npm')
  })

  // Windows shims are `.cmd` scripts in a directory that says nothing about
  // their origin (`%APPDATA%\npm`), so the script's target decides.
  test('reads a shim script for its target', () => {
    const root = tempRoot()
    const npmShim = writeExecutable(path.join(root, 'npm'), 'pnpm.cmd', '@ECHO off\r\n"%~dp0\\node.exe" "%~dp0\\node_modules\\pnpm\\bin\\pnpm.cjs" %*\r\n')
    assert.equal(detectInstallOrigin(npmShim), 'npm')

    const corepackShim = writeExecutable(path.join(root, 'node'), 'pnpm', '#!/bin/sh\nexec corepack pnpm "$@"\n')
    assert.equal(detectInstallOrigin(corepackShim), 'corepack')

    const plain = writeExecutable(path.join(root, 'plain'), 'pnpm', '#!/bin/sh\nexit 0\n')
    assert.equal(detectInstallOrigin(plain), 'unknown')
  })
})

describe('pnpmHomeDir', () => {
  test('takes PNPM_HOME first, then XDG_DATA_HOME, then the platform default', () => {
    assert.equal(pnpmHomeDir({ PNPM_HOME: '/opt/pnpm', XDG_DATA_HOME: '/data' }, 'linux'), '/opt/pnpm')
    assert.equal(pnpmHomeDir({ XDG_DATA_HOME: '/data' }, 'darwin'), path.join('/data', 'pnpm'))
    assert.equal(pnpmHomeDir({}, 'darwin'), path.join(os.homedir(), 'Library', 'pnpm'))
    assert.equal(pnpmHomeDir({}, 'linux'), path.join(os.homedir(), '.local', 'share', 'pnpm'))
    assert.equal(pnpmHomeDir({ LOCALAPPDATA: 'C:\\Users\\me\\AppData\\Local' }, 'win32'), path.join('C:\\Users\\me\\AppData\\Local', 'pnpm'))
  })
})

describe('renderShadowingPnpmWarning', () => {
  test('names the removal command and the PATH order', () => {
    const home = path.join('/home', 'me', '.local', 'share', 'pnpm')
    const bin = path.join(home, 'bin')
    const executable = path.join('/opt', 'homebrew', 'bin', 'pnpm')
    assert.equal(
      renderShadowingPnpmWarning({ executable, origin: 'npm', binOnPath: true }, home),
      `Warning: "pnpm" on PATH is ${executable} (installed with npm), which comes before ${bin}.\n` +
      `Your shell keeps running that pnpm, not the one pnpm installed to ${bin}.\n` +
      `To finish switching, run "npm uninstall -g pnpm" or move ${bin} ahead of ${path.dirname(executable)} in PATH.`
    )
  })

  test('only reorders the PATH for an unknown origin', () => {
    const home = path.join('/home', 'me', '.local', 'share', 'pnpm')
    const bin = path.join(home, 'bin')
    const executable = path.join('/usr', 'local', 'bin', 'pnpm')
    assert.equal(
      renderShadowingPnpmWarning({ executable, origin: 'unknown', binOnPath: false }, home),
      `Warning: "pnpm" on PATH is ${executable} (not installed by pnpm), and ${bin} is not on PATH yet.\n` +
      `Once a new shell adds it, it has to come first: move ${bin} ahead of ${path.dirname(executable)} in PATH.`
    )
  })
})
