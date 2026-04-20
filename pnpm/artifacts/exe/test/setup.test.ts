import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'

// eslint-disable-next-line @typescript-eslint/ban-ts-comment
// @ts-expect-error — JS helper without type declarations
import { exePlatformPkgName } from '../platform-pkg-name.js'

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

describe('exePlatformPkgName', () => {
  test('appends -musl for linux + musl libc family', () => {
    expect(exePlatformPkgName('linux', 'x64', 'musl')).toBe('@pnpm/exe.linux-x64-musl')
    expect(exePlatformPkgName('linux', 'arm64', 'musl')).toBe('@pnpm/exe.linux-arm64-musl')
  })

  test('does not append -musl when libc is glibc or unknown', () => {
    expect(exePlatformPkgName('linux', 'x64', 'glibc')).toBe('@pnpm/exe.linux-x64')
    expect(exePlatformPkgName('linux', 'arm64', null)).toBe('@pnpm/exe.linux-arm64')
  })

  test('libc is irrelevant on non-linux platforms', () => {
    expect(exePlatformPkgName('darwin', 'arm64', 'musl')).toBe('@pnpm/exe.darwin-arm64')
    expect(exePlatformPkgName('darwin', 'x64', null)).toBe('@pnpm/exe.darwin-x64')
    expect(exePlatformPkgName('win32', 'x64', 'musl')).toBe('@pnpm/exe.win32-x64')
  })

  test('normalizes ia32 to x86 on win32 only', () => {
    expect(exePlatformPkgName('win32', 'ia32', null)).toBe('@pnpm/exe.win32-x86')
    expect(exePlatformPkgName('linux', 'ia32', null)).toBe('@pnpm/exe.linux-ia32')
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
})
