import fs, { type Stats } from 'node:fs'
import path from 'node:path'

import { createShortHash } from '@pnpm/crypto.hash'
import { engineName } from '@pnpm/engine.runtime.system-version'
import { isError } from '@pnpm/error'
import { lexCompare } from '@pnpm/text.ordinal-comparator'
import type { SupportedArchitectures } from '@pnpm/types'

export interface DlxCache {
  cacheLink: string
  cacheExists: boolean
  cachedDir: string
}

export function findCache (opts: {
  packages: string[]
  cacheDir: string
  dlxCacheMaxAge: number
  registriesByScope: Record<string, string>
  allowBuild?: string[]
  supportedArchitectures?: SupportedArchitectures
  nodeVersion?: string
}): DlxCache {
  const dlxCommandCacheDir = createDlxCommandCacheDir(opts)
  const cacheLink = path.join(dlxCommandCacheDir, 'pkg')
  const cachedDir = getValidCacheDir(cacheLink, opts.dlxCacheMaxAge)
  return {
    cacheLink,
    cachedDir: cachedDir ?? getPrepareDir(dlxCommandCacheDir),
    cacheExists: cachedDir != null,
  }
}

function createDlxCommandCacheDir (
  opts: {
    packages: string[]
    registriesByScope: Record<string, string>
    cacheDir: string
    allowBuild?: string[]
    supportedArchitectures?: SupportedArchitectures
    nodeVersion?: string
  }
): string {
  const dlxCacheDir = path.resolve(opts.cacheDir, 'dlx')
  const cacheKey = createCacheKey(opts)
  const cachePath = path.join(dlxCacheDir, cacheKey)
  fs.mkdirSync(cachePath, { recursive: true })
  return cachePath
}

export function createCacheKey (opts: {
  packages: string[]
  registriesByScope: Record<string, string>
  allowBuild?: string[]
  supportedArchitectures?: SupportedArchitectures
  nodeVersion?: string
}): string {
  const sortedPkgs = [...opts.packages].sort(lexCompare)
  const sortedRegistries = Object.entries(opts.registriesByScope).sort(([k1], [k2]) => lexCompare(k1, k2))
  const args: unknown[] = [sortedPkgs, sortedRegistries]
  if (opts.allowBuild?.length) {
    args.push({ allowBuild: opts.allowBuild.sort(lexCompare) })
  }
  if (opts.supportedArchitectures) {
    const supportedArchitecturesKeys = ['cpu', 'libc', 'os'] as const satisfies Array<keyof SupportedArchitectures>
    for (const key of supportedArchitecturesKeys) {
      const value = opts.supportedArchitectures[key]
      if (!value?.length) continue
      args.push({
        supportedArchitectures: {
          [key]: [...new Set(value)].sort(lexCompare),
        },
      })
    }
  }
  // Packages built by lifecycle scripts, native addons especially, only load
  // on the platform, architecture, and Node.js major they were built for.
  args.push({ engine: engineName(opts.nodeVersion) })
  const hashStr = JSON.stringify(args)
  // A short (truncated) hash keeps the dlx cache path short. The full
  // virtual-store path below it (`<key>/<prepare>/node_modules/.pnpm/<pkgId>/
  // node_modules/<pkg>`) can otherwise blow past Windows' MAX_PATH (260) and
  // make lifecycle scripts fail with a `spawn cmd.exe ENOENT` (the cwd no
  // longer resolves). 128 bits is ample collision resistance for a cache key.
  return createShortHash(hashStr)
}

export function getValidCacheDir (cacheLink: string, dlxCacheMaxAge: number): string | undefined {
  let stats: Stats
  let target: string
  try {
    stats = fs.lstatSync(cacheLink)
    if (stats.isSymbolicLink()) {
      target = fs.realpathSync(cacheLink)
      if (!target) return undefined
    } else {
      return undefined
    }
  } catch (err) {
    if (isError(err) && 'code' in err && err.code === 'ENOENT') {
      return undefined
    }
    throw err
  }
  const isValid = fs.existsSync(path.join(target, 'pnpm-lock.yaml')) &&
    stats.mtime.getTime() + dlxCacheMaxAge * 60_000 >= new Date().getTime()
  return isValid ? target : undefined
}

function getPrepareDir (cachePath: string): string {
  // base36 (vs hex) keeps this segment short — see createCacheKey for why dlx
  // path length matters on Windows. time+pid stays unique across concurrent
  // dlx processes and across a process's own retries of a failed install.
  const name = `${Date.now().toString(36)}-${process.pid.toString(36)}`
  return path.join(cachePath, name)
}
