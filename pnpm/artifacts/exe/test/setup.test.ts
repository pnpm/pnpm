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

  // 3. Verify pnpm and pn are hardlinks to the platform binary
  const pnpmBin = path.join(exeDir, isWindows ? 'pnpm.exe' : 'pnpm')
  const pnBin = path.join(exeDir, isWindows ? 'pn.exe' : 'pn')
  const platformBin = path.join(
    exeDir, 'node_modules', '@pnpm', `${platform}-${arch}`,
    isWindows ? 'pnpm.exe' : 'pnpm'
  )

  expect(fs.existsSync(pnpmBin)).toBe(true)
  expect(fs.existsSync(pnBin)).toBe(true)

  // All three should share the same inode (hardlinks)
  const platformIno = fs.statSync(platformBin).ino
  expect(fs.statSync(pnpmBin).ino).toBe(platformIno)
  expect(fs.statSync(pnBin).ino).toBe(platformIno)

  // 4. Verify pnpx and pnx are shell scripts that delegate to pnpm dlx
  if (!isWindows) {
    const pnpxContent = fs.readFileSync(path.join(exeDir, 'pnpx'), 'utf8')
    expect(pnpxContent).toBe('#!/bin/sh\nexec pnpm dlx "$@"\n')
    const pnxContent = fs.readFileSync(path.join(exeDir, 'pnx'), 'utf8')
    expect(pnxContent).toBe('#!/bin/sh\nexec pnpm dlx "$@"\n')

    // Verify they're executable
    const pnpxMode = fs.statSync(path.join(exeDir, 'pnpx')).mode
    expect(pnpxMode & 0o111).not.toBe(0)
    const pnxMode = fs.statSync(path.join(exeDir, 'pnx')).mode
    expect(pnxMode & 0o111).not.toBe(0)
  } else {
    expect(fs.existsSync(path.join(exeDir, 'pnpx.cmd'))).toBe(true)
    expect(fs.existsSync(path.join(exeDir, 'pnx.cmd'))).toBe(true)
    expect(fs.readFileSync(path.join(exeDir, 'pnpx.cmd'), 'utf8')).toBe('@echo off\npnpm dlx %*\n')
    expect(fs.readFileSync(path.join(exeDir, 'pnx.cmd'), 'utf8')).toBe('@echo off\npnpm dlx %*\n')
  }
})
