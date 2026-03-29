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

const platformBin = path.join(
  exeDir, 'node_modules', '@pnpm', `${platform}-${arch}`,
  isWindows ? 'pnpm.exe' : 'pnpm'
)
const hasPlatformBinary = fs.existsSync(platformBin)

test('prepare writes correct content for all bin files', () => {
  execFileSync(process.execPath, [path.join(exeDir, 'prepare.js')], { cwd: exeDir })

  // pnpm and pn should be placeholders (replaced by setup.js with hardlinks)
  for (const name of ['pnpm', 'pn']) {
    expect(fs.readFileSync(path.join(exeDir, name), 'utf8')).toBe('This file intentionally left blank')
  }

  // pnpx and pnx should be real shell scripts
  for (const name of ['pnpx', 'pnx']) {
    expect(fs.readFileSync(path.join(exeDir, name), 'utf8')).toBe('#!/bin/sh\nexec pnpm dlx "$@"\n')
    if (!isWindows) {
      expect(fs.statSync(path.join(exeDir, name)).mode & 0o111).not.toBe(0)
    }
  }

  // Windows wrappers should exist
  for (const name of ['pnpx', 'pnx']) {
    expect(fs.readFileSync(path.join(exeDir, name + '.cmd'), 'utf8')).toBe('@echo off\npnpm dlx %*\n')
    expect(fs.readFileSync(path.join(exeDir, name + '.ps1'), 'utf8')).toBe('pnpm dlx @args\n')
  }
});

(hasPlatformBinary ? test : test.skip)('setup.js creates hardlinks for pnpm and pn', () => {
  // Run prepare first to simulate the published tarball state
  execFileSync(process.execPath, [path.join(exeDir, 'prepare.js')], { cwd: exeDir })
  execFileSync(process.execPath, [path.join(exeDir, 'setup.js')], { cwd: exeDir })

  const pnpmBin = path.join(exeDir, isWindows ? 'pnpm.exe' : 'pnpm')
  const pnBin = path.join(exeDir, isWindows ? 'pn.exe' : 'pn')

  // pnpm and pn should be hardlinks to the platform binary
  expect(fs.statSync(pnpmBin).ino).toBe(fs.statSync(platformBin).ino)
  expect(fs.statSync(pnBin).ino).toBe(fs.statSync(platformBin).ino)
})
