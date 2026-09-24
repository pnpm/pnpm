import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { describe, expect, test } from '@jest/globals'
import { detectInstallOrigin, findShadowingPnpm, renderShadowingPnpmWarning } from '@pnpm/engine.pm.commands'

// The file name a PATH lookup for `pnpm` accepts on this platform.
const PNPM = process.platform === 'win32' ? 'pnpm.cmd' : 'pnpm'

function tempRoot (): string {
  return fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-shadowing-'))
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
    expect(findShadowingPnpm(path.join(tempRoot(), 'bin'), undefined)).toBeUndefined()
  })

  test('nothing shadows when the global bin comes first', () => {
    const root = tempRoot()
    const globalBin = path.join(root, 'bin')
    writeExecutable(globalBin, PNPM)
    const other = path.join(root, 'other')
    writeExecutable(other, PNPM)

    expect(findShadowingPnpm(globalBin, pathEnv([globalBin, other]))).toBeUndefined()
  })

  test('nothing shadows when no pnpm is on the PATH', () => {
    const root = tempRoot()
    const empty = path.join(root, 'empty')
    fs.mkdirSync(empty)

    expect(findShadowingPnpm(path.join(root, 'bin'), pathEnv([empty]))).toBeUndefined()
  })

  test('a pnpm ahead of the global bin shadows it', () => {
    const root = tempRoot()
    const globalBin = path.join(root, 'bin')
    writeExecutable(globalBin, PNPM)
    const other = path.join(root, 'other')
    const executable = writeExecutable(other, PNPM)

    expect(findShadowingPnpm(globalBin, pathEnv([other, globalBin]))).toStrictEqual({
      executable,
      origin: 'unknown',
      globalBinOnPath: true,
    })
  })

  test('a pnpm shadows a global bin that is not on the PATH yet', () => {
    const root = tempRoot()
    const other = path.join(root, 'other')
    const executable = writeExecutable(other, PNPM)

    expect(findShadowingPnpm(path.join(root, 'bin'), pathEnv([other]))).toStrictEqual({
      executable,
      origin: 'unknown',
      globalBinOnPath: false,
    })
  })

  // The v10 layout links `pnpm` straight into the pnpm home directory, and
  // CI actions do the same; that is pnpm's own, not another installer's.
  test('a pnpm in the pnpm home directory is pnpm itself', () => {
    const root = tempRoot()
    writeExecutable(root, PNPM)

    expect(findShadowingPnpm(path.join(root, 'bin'), pathEnv([root]))).toBeUndefined()
  })

  test('a symlink into the global bin is pnpm itself', () => {
    const root = tempRoot()
    const globalBin = path.join(root, 'bin')
    const executable = writeExecutable(globalBin, PNPM)
    const links = path.join(root, 'links')
    fs.mkdirSync(links)
    fs.symlinkSync(executable, path.join(links, PNPM))

    expect(findShadowingPnpm(globalBin, pathEnv([links, globalBin]))).toBeUndefined()
  })
})

describe('detectInstallOrigin', () => {
  test.each([
    ['opt/homebrew/lib/node_modules/pnpm/bin/pnpm.mjs', 'npm'],
    ['opt/homebrew/Cellar/pnpm/12.6.0/bin/pnpm', 'homebrew'],
    ['usr/local/lib/node_modules/corepack/shims/pnpm', 'corepack'],
    ['home/me/.volta/bin/pnpm', 'volta'],
    ['Users/me/scoop/shims/pnpm.cmd', 'scoop'],
    ['home/me/bin/pnpm', 'unknown'],
  ])('is read off the path %s', (relative, expected) => {
    expect(detectInstallOrigin(path.join(tempRoot(), relative))).toBe(expected)
  })

  test('follows a symlink to its target', () => {
    const root = tempRoot()
    const target = writeExecutable(path.join(root, 'lib', 'node_modules', 'pnpm', 'bin'), 'pnpm.mjs')
    const link = path.join(root, 'bin', 'pnpm')
    fs.mkdirSync(path.dirname(link))
    fs.symlinkSync(target, link)

    expect(detectInstallOrigin(link)).toBe('npm')
  })

  // Windows shims are `.cmd` scripts in a directory that says nothing about
  // their origin (`%APPDATA%\npm`), so the script's target decides.
  test('reads a shim script for its target', () => {
    const root = tempRoot()
    const npmShim = writeExecutable(path.join(root, 'npm'), 'pnpm.cmd', '@ECHO off\r\n"%~dp0\\node.exe" "%~dp0\\node_modules\\pnpm\\bin\\pnpm.cjs" %*\r\n')
    expect(detectInstallOrigin(npmShim)).toBe('npm')

    const corepackShim = writeExecutable(path.join(root, 'node'), 'pnpm', '#!/bin/sh\nexec corepack pnpm "$@"\n')
    expect(detectInstallOrigin(corepackShim)).toBe('corepack')

    const plain = writeExecutable(path.join(root, 'plain'), 'pnpm', '#!/bin/sh\nexit 0\n')
    expect(detectInstallOrigin(plain)).toBe('unknown')
  })
})

describe('renderShadowingPnpmWarning', () => {
  test('names the removal command and the PATH order', () => {
    const globalBin = path.join('/home', 'me', '.local', 'share', 'pnpm', 'bin')
    const executable = path.join('/opt', 'homebrew', 'bin', 'pnpm')
    expect(renderShadowingPnpmWarning({ executable, origin: 'npm', globalBinOnPath: true }, globalBin)).toBe(
      `"pnpm" on PATH is ${executable} (installed with npm), which comes before ${globalBin}. ` +
      `Your shell keeps running that pnpm, not the one pnpm installed to ${globalBin}. ` +
      `To finish switching, run "npm uninstall -g pnpm" or move ${globalBin} ahead of ${path.dirname(executable)} in PATH.`
    )
  })

  test('only reorders the PATH for an unknown origin', () => {
    const globalBin = path.join('/home', 'me', '.local', 'share', 'pnpm', 'bin')
    const executable = path.join('/usr', 'local', 'bin', 'pnpm')
    expect(renderShadowingPnpmWarning({ executable, origin: 'unknown', globalBinOnPath: false }, globalBin)).toBe(
      `"pnpm" on PATH is ${executable} (not installed by pnpm), and ${globalBin} is not on PATH yet. ` +
      `Once a new shell adds it, it has to come first: move ${globalBin} ahead of ${path.dirname(executable)} in PATH.`
    )
  })
})
