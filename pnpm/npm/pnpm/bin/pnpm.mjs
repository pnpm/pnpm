#!/usr/bin/env node
import { spawnSync } from 'node:child_process'
import { createRequire } from 'node:module'
import fs from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const require = createRequire(import.meta.url)

const PLATFORMS = {
  win32: {
    x64: '@pnpm/exe.win32-x64/pnpm.exe',
    arm64: '@pnpm/exe.win32-arm64/pnpm.exe',
  },
  darwin: {
    x64: '@pnpm/exe.darwin-x64/pnpm',
    arm64: '@pnpm/exe.darwin-arm64/pnpm',
  },
  linux: {
    x64: {
      glibc: '@pnpm/exe.linux-x64/pnpm',
      musl: '@pnpm/exe.linux-x64-musl/pnpm',
    },
    arm64: {
      glibc: '@pnpm/exe.linux-arm64/pnpm',
      musl: '@pnpm/exe.linux-arm64-musl/pnpm',
    },
  },
}

export function getBinCandidates ({ platform = process.platform, arch = process.arch, libc = detectLinuxLibc() } = {}) {
  const platformEntry = PLATFORMS?.[platform]?.[arch]

  if (platformEntry == null) {
    return []
  }
  if (typeof platformEntry === 'string') {
    return [platformEntry]
  }

  const order = libc === 'musl' ? ['musl', 'glibc'] : ['glibc', 'musl']
  return order.map((libc) => platformEntry[libc])
}

export function resolveNativeBinary ({ candidates = getBinCandidates(), requireResolve = require.resolve } = {}) {
  for (const target of candidates) {
    try {
      return requireResolve(target)
    } catch {}
  }
  return null
}

export function runPnpm ({ argv = process.argv.slice(2), spawn = spawnSync } = {}) {
  const candidates = getBinCandidates()
  if (candidates.length === 0) {
    fail(`pnpm does not ship a prebuilt binary for ${process.platform}-${process.arch}.`)
  }

  const nativeBinary = resolveNativeBinary({ candidates })
  if (nativeBinary == null) {
    const pkgName = candidates[0].split('/').slice(0, 2).join('/')
    fail(
      `The "${pkgName}" package is not installed, so pnpm has no native binary to run.\n` +
      'If your package manager skipped optional dependencies, enable them and reinstall.'
    )
  }

  const result = spawn(nativeBinary, argv, { stdio: 'inherit' })
  if (result.error != null) {
    fail(`Could not run the pnpm binary at ${nativeBinary}: ${result.error.message}`)
  }
  if (result.signal != null) {
    process.kill(process.pid, result.signal)
    return
  }
  process.exit(result.status ?? 1)
}

function fail (message) {
  console.error(message)
  process.exit(1)
}

function detectLinuxLibc () {
  if (process.platform !== 'linux') {
    return null
  }

  try {
    return process.report?.getReport().header.glibcVersionRuntime ? 'glibc' : 'musl'
  } catch {
    return null
  }
}

function isMain () {
  if (process.argv[1] == null) {
    return false
  }
  return fs.realpathSync(fileURLToPath(import.meta.url)) === fs.realpathSync(path.resolve(process.argv[1]))
}

if (isMain()) {
  runPnpm()
}
