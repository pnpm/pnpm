import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'

const exeDir = path.resolve(import.meta.dirname, '..')
const platform = process.platform
const arch = platform === 'win32' && process.arch === 'ia32' ? 'x86' : process.arch
const isWindows = platform === 'win32'
// The test doesn't create a musl libc marker, so setup.js's detect-libc call
// reports the host's native libc; on a glibc Linux CI box that resolves to the
// non-musl package name. For non-Linux hosts there is no libc suffix.
const platformBin = path.join(
  exeDir, 'node_modules', '@pnpm', `exe.${platform}-${arch}`,
  isWindows ? 'pnpm.exe' : 'pnpm'
)
const hasPlatformBinary = fs.existsSync(platformBin)

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
})
