import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'

const exeDir = path.resolve(import.meta.dirname, '..')
const platform = process.platform === 'win32'
  ? 'win'
  : process.platform === 'darwin'
    ? 'macos'
    : process.platform
const arch = platform === 'win' && process.arch === 'ia32' ? 'x86' : process.arch
const isWindows = platform === 'win'

test('prepare then setup creates working binaries for all commands', () => {
  // 1. Run prepare.js — simulates the publish step that writes placeholders
  execFileSync(process.execPath, [path.join(exeDir, 'prepare.js')], { cwd: exeDir })

  // All bin files should be placeholders now
  for (const name of ['pnpm', 'pn', 'pnpx', 'pnx']) {
    expect(fs.readFileSync(path.join(exeDir, name), 'utf8')).toBe('This file intentionally left blank')
  }

  // 2. Run setup.js — simulates the preinstall step on a real install
  execFileSync(process.execPath, [path.join(exeDir, 'setup.js')], { cwd: exeDir })

  // 3. Verify pnpm is a hardlink to the platform binary
  const pnpmBin = path.join(exeDir, isWindows ? 'pnpm.exe' : 'pnpm')
  const platformBin = path.join(
    exeDir, 'node_modules', '@pnpm', `${platform}-${arch}`,
    isWindows ? 'pnpm.exe' : 'pnpm'
  )

  expect(fs.existsSync(pnpmBin)).toBe(true)
  expect(fs.statSync(pnpmBin).ino).toBe(fs.statSync(platformBin).ino)

  // 4. Verify pn is a hardlink to the platform binary
  const pnBin = path.join(exeDir, isWindows ? 'pn.exe' : 'pn')
  expect(fs.existsSync(pnBin)).toBe(true)
  expect(fs.statSync(pnBin).ino).toBe(fs.statSync(platformBin).ino)

  // 5. Verify pnpx and pnx are shell scripts that delegate to pnpm dlx
  if (!isWindows) {
    expect(fs.readFileSync(path.join(exeDir, 'pnpx'), 'utf8')).toBe('#!/bin/sh\nexec pnpm dlx "$@"\n')
    expect(fs.readFileSync(path.join(exeDir, 'pnx'), 'utf8')).toBe('#!/bin/sh\nexec pnpm dlx "$@"\n')

    // Verify they're executable
    for (const name of ['pnpx', 'pnx']) {
      expect(fs.statSync(path.join(exeDir, name)).mode & 0o111).not.toBe(0)
    }
  } else {
    expect(fs.readFileSync(path.join(exeDir, 'pnpx.cmd'), 'utf8')).toBe('@echo off\npnpm dlx %*\n')
    expect(fs.readFileSync(path.join(exeDir, 'pnx.cmd'), 'utf8')).toBe('@echo off\npnpm dlx %*\n')
  }
})
