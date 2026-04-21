import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'

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
})
