import fs, { type Stats } from 'node:fs'
import path from 'node:path'

import { parseOverrides } from '@pnpm/config.parse-overrides'
import { createShortHash } from '@pnpm/crypto.hash'
import { engineName } from '@pnpm/engine.runtime.system-version'
import { isError } from '@pnpm/error'
import { add } from '@pnpm/installing.commands'
import { lexCompare } from '@pnpm/text.ordinal-comparator'
import type { PackageExtension, SupportedArchitectures, TrustPolicy } from '@pnpm/types'

export interface DlxCache {
  cacheLink: string
  cacheExists: boolean
  cachedDir: string
}

export interface DlxCacheKeyOptions {
  packages: string[]
  registriesByScope: Record<string, string>
  allowBuild?: string[]
  supportedArchitectures?: SupportedArchitectures
  nodeVersion?: string
  overrides?: Record<string, string>
  packageExtensions?: Record<string, PackageExtension>
  minimumReleaseAge?: number
  minimumReleaseAgeExclude?: string[]
  minimumReleaseAgeIgnoreMissingTime?: boolean
  minimumReleaseAgeStrict?: boolean
  trustPolicy?: TrustPolicy
  trustPolicyExclude?: string[]
  trustPolicyIgnoreAfter?: number
  trustLockfile?: boolean
  blockExoticSubdeps?: boolean
}

export interface FindDlxCacheOptions extends DlxCacheKeyOptions {
  cacheDir: string
  dlxCacheMaxAge: number
}

export function findCache (opts: FindDlxCacheOptions): DlxCache {
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
  opts: FindDlxCacheOptions
): string {
  const dlxCacheDir = path.resolve(opts.cacheDir, 'dlx')
  const cacheKey = createCacheKey(opts)
  const cachePath = path.join(dlxCacheDir, cacheKey)
  fs.mkdirSync(cachePath, { recursive: true })
  return cachePath
}

export function createCacheKey (opts: DlxCacheKeyOptions): string {
  const sortedPkgs = [...opts.packages].sort(lexCompare)
  const sortedRegistries = Object.entries(opts.registriesByScope).sort(([k1], [k2]) => lexCompare(k1, k2))
  const args: unknown[] = [sortedPkgs, sortedRegistries]
  const normalizedAllowBuild = normalizeAllowBuild(opts.allowBuild)
  if (normalizedAllowBuild.length) {
    args.push({ allowBuild: normalizedAllowBuild })
  }
  if (opts.supportedArchitectures) {
    const supportedArchitecturesKeys = ['cpu', 'libc', 'os'] as const satisfies Array<keyof SupportedArchitectures>
    for (const key of supportedArchitecturesKeys) {
      const value = opts.supportedArchitectures[key]
      if (!value?.length) continue
      args.push({
        supportedArchitectures: {
          [key]: sortUnique(value),
        },
      })
    }
  }
  appendPolicyCacheKeyInputs(args, opts)
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

function normalizeAllowBuild (allowBuild: string[] | undefined): string[] {
  if (!allowBuild?.length) return []
  const verdicts = new Map<string, boolean>()
  for (const selector of allowBuild) {
    const { name, allowed } = add.parseAllowBuildSelector(selector)
    verdicts.set(name, allowed)
  }
  return Array.from(verdicts, ([name, allowed]) => allowed ? name : `!${name}`).sort(lexCompare)
}

function appendPolicyCacheKeyInputs (args: unknown[], opts: DlxCacheKeyOptions): void {
  // Entry order breaks ties when two override selectors cover intersecting
  // target ranges, so the cache key preserves it.
  const overrideEntries = opts.overrides ? Object.entries(opts.overrides) : []
  if (overrideEntries.length) {
    args.push({ overrides: overrideEntries })
  }
  if (opts.packageExtensions && Object.keys(opts.packageExtensions).length > 0) {
    // Selector order decides which extension wins a conflicting dependency,
    // so keep the outer order while canonicalizing each extension's content.
    args.push({
      packageExtensions: Object.entries(opts.packageExtensions).map(([selector, extension]) => [
        selector,
        canonicalizeJsonValue(extension),
      ]),
    })
  }
  const minimumReleaseAge = buildMinimumReleaseAgeInput(opts)
  if (minimumReleaseAge != null) {
    args.push({ minimumReleaseAge })
  }
  const trustPolicy = buildTrustPolicyInput(opts)
  if (trustPolicy != null) {
    args.push({ trustPolicy })
  }
  if (opts.trustLockfile === true) {
    args.push({ trustLockfile: true })
  }
  if (opts.blockExoticSubdeps === false) {
    args.push({ blockExoticSubdeps: false })
  }
}

function canonicalizeJsonValue (value: unknown): unknown {
  if (Array.isArray(value)) {
    return value.map(canonicalizeJsonValue)
  }
  if (value != null && typeof value === 'object') {
    const sortedEntries: Array<[string, unknown]> = []
    for (const key of Object.keys(value).sort(lexCompare)) {
      const child = (value as Record<string, unknown>)[key]
      if (child !== undefined) {
        sortedEntries.push([key, canonicalizeJsonValue(child)])
      }
    }
    return Object.fromEntries(sortedEntries)
  }
  return value
}

function buildMinimumReleaseAgeInput (opts: DlxCacheKeyOptions): Record<string, unknown> | undefined {
  if (opts.minimumReleaseAge == null || opts.minimumReleaseAge <= 0) return undefined
  const entry: Record<string, unknown> = {
    minutes: opts.minimumReleaseAge,
    strict: opts.minimumReleaseAgeStrict === true,
    ignoreMissingTime: Boolean(opts.minimumReleaseAgeIgnoreMissingTime),
  }
  if (opts.minimumReleaseAgeExclude?.length) {
    entry.exclude = sortUnique(opts.minimumReleaseAgeExclude)
  }
  return entry
}

function buildTrustPolicyInput (opts: DlxCacheKeyOptions): Record<string, unknown> | undefined {
  if (opts.trustPolicy !== 'no-downgrade') return undefined
  const entry: Record<string, unknown> = {
    policy: opts.trustPolicy,
  }
  if (opts.trustPolicyExclude?.length) {
    entry.exclude = sortUnique(opts.trustPolicyExclude)
  }
  if (opts.trustPolicyIgnoreAfter != null) {
    entry.ignoreAfter = opts.trustPolicyIgnoreAfter
  }
  return entry
}

function sortUnique (values: string[]): string[] {
  return [...new Set(values)].sort(lexCompare)
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

export function resolveDlxOverrides (
  overrides: Record<string, string> | undefined,
  catalogs: Parameters<typeof parseOverrides>[1]
): Record<string, string> | undefined {
  if (!overrides) return undefined
  if (!Object.values(overrides).some((spec) => spec.startsWith('catalog:'))) {
    return overrides
  }
  return Object.fromEntries(
    parseOverrides(overrides, catalogs).map(({ selector, newBareSpecifier }) => [selector, newBareSpecifier])
  )
}
