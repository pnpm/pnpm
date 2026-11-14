import fs from 'node:fs'
import path from 'node:path'

import { isError, PnpmError } from '@pnpm/error'

import { normalizePackageName } from '../tarball/safeTarballFilename.js'

export function assertValidTarballVersion (version: string, reportedVersion: string = version): void {
  if (
    !version ||
    version === '.' ||
    version === '..' ||
    version.includes('/') ||
    version.includes('\\') ||
    version.includes(':')
  ) {
    throw new PnpmError('INVALID_PACKAGE_VERSION', `Invalid package version "${reportedVersion}".`)
  }
}

export function resolvePackOutput (
  params: {
    dir: string
    outputBaseDir?: string
    out?: string
    packDestination?: string
    pathImpl?: path.PlatformPath
    publishedName: string
    publishedVersion: string
  }
): { destDir: string, outputPath: string, tarballName: string } {
  const pathImpl = params.pathImpl ?? path
  const baseDir = params.outputBaseDir ?? params.dir
  const normalizedName = normalizePackageName(params.publishedName)
  const publishedVersion = stripBuildMetadata(params.publishedVersion)
  const { tarballName, destination } = resolveTarballAndDestination({
    out: params.out,
    packDestination: params.packDestination,
    pathImpl,
    normalizedName,
    publishedVersion,
  })
  const destDir = resolveRelativeTarget(baseDir, destination, pathImpl)
  return {
    destDir,
    outputPath: resolveRelativeTarget(destDir, tarballName, pathImpl),
    tarballName,
  }
}

export function stripBuildMetadata (version: string): string {
  const plusIndex = version.indexOf('+')
  return plusIndex === -1 ? version : version.slice(0, plusIndex)
}

function resolveTarballAndDestination (
  params: {
    out?: string
    packDestination?: string
    pathImpl: path.PlatformPath
    normalizedName: string
    publishedVersion: string
  }
): { tarballName: string, destination?: string } {
  const { out, packDestination, pathImpl, normalizedName, publishedVersion } = params
  if (out == null) {
    return {
      destination: packDestination,
      tarballName: `${normalizedName}-${publishedVersion}.tgz`,
    }
  }
  if (packDestination) {
    throw new PnpmError('INVALID_OPTION', 'Cannot use --pack-destination and --out together')
  }
  const preparedOut = out.replaceAll('%s', normalizedName).replaceAll('%v', publishedVersion)
  const parsedOut = pathImpl.parse(preparedOut)
  if (!parsedOut.base || parsedOut.base === '.' || parsedOut.base === '..') {
    throw new PnpmError('INVALID_OPTION', `Invalid --out value "${out}": it does not resolve to a file name`)
  }
  return {
    destination: parsedOut.dir || undefined,
    tarballName: parsedOut.base,
  }
}

export type PackDestinationCache = Map<string, Promise<string | undefined>>

export async function canonicalizePackDestination (
  destDir: string,
  cache?: PackDestinationCache
): Promise<string> {
  const direct = await probeRealpath(destDir, cache)
  if (direct != null) return direct
  return resolveMissingDestination(resolveRelativeTarget(process.cwd(), destDir), cache)
}

async function resolveMissingDestination (
  anchoredDir: string,
  cache?: PackDestinationCache
): Promise<string> {
  const { root } = path.parse(anchoredDir)
  const segments = splitPathSegments(anchoredDir.slice(root.length))
    .filter((segment) => segment !== '' && segment !== '.')
  return walkDestinationSegments({
    cache,
    index: 0,
    segments,
    state: { current: root || '.', missingTail: false },
  })
}

function splitPathSegments (relativePath: string): string[] {
  return relativePath.split(process.platform === 'win32' ? /[/\\]+/ : /\/+/)
}

async function walkDestinationSegments (params: {
  cache?: PackDestinationCache
  index: number
  segments: string[]
  state: { current: string, missingTail: boolean }
}): Promise<string> {
  const { cache, index, segments, state } = params
  if (index >= segments.length) return state.current
  const nextState = await stepDestinationSegment({
    cache,
    current: state.current,
    missingTail: state.missingTail,
    segment: segments[index],
  })
  return walkDestinationSegments({ cache, index: index + 1, segments, state: nextState })
}

async function stepDestinationSegment (params: {
  cache?: PackDestinationCache
  current: string
  missingTail: boolean
  segment: string
}): Promise<{ current: string, missingTail: boolean }> {
  const { cache, current, missingTail, segment } = params
  if (segment === '..') {
    const parent = path.dirname(current)
    return missingTail ? tryRealpath(parent, cache) : { current: parent, missingTail: false }
  }
  const next = path.join(current, segment)
  return missingTail ? { current: next, missingTail: true } : tryRealpath(next, cache)
}

async function tryRealpath (
  candidate: string,
  cache?: PackDestinationCache
): Promise<{ current: string, missingTail: boolean }> {
  const resolved = await probeRealpath(candidate, cache)
  return resolved != null
    ? { current: resolved, missingTail: false }
    : { current: candidate, missingTail: true }
}

async function probeRealpath (
  candidate: string,
  cache?: PackDestinationCache
): Promise<string | undefined> {
  let pending = cache?.get(candidate)
  if (pending == null) {
    pending = readRealpathOrUndefined(candidate)
    cache?.set(candidate, pending)
  }
  try {
    return await pending
  } catch (err: unknown) {
    cache?.delete(candidate)
    throw err
  }
}

async function readRealpathOrUndefined (candidate: string): Promise<string | undefined> {
  try {
    return await fs.promises.realpath(candidate)
  } catch (err: unknown) {
    if (isMissingPathError(err)) return undefined
    throw err
  }
}

function isMissingPathError (err: unknown): boolean {
  return isError(err) && 'code' in err && (err.code === 'ENOENT' || err.code === 'ENOTDIR')
}

/**
 * Joins a relative `targetPath` onto `baseDir` without lexically collapsing
 * `..` segments, so `symlink/..` resolves through the symlink target on
 * filesystem access.
 */
export function resolveRelativeTarget (
  baseDir: string,
  targetPath: string | undefined,
  pathImpl: path.PlatformPath = path
): string {
  if (!targetPath) return baseDir
  if (pathImpl === path.win32) {
    const resolved = resolveWin32Target(baseDir, targetPath)
    if (resolved.done != null) return resolved.done
    return joinUnnormalizedSegments(resolved.baseDir, resolved.relativeTarget, path.win32)
  }
  if (pathImpl.isAbsolute(targetPath)) return targetPath
  return joinUnnormalizedSegments(baseDir, targetPath, pathImpl)
}

function joinUnnormalizedSegments (
  baseDir: string,
  relativeTarget: string,
  pathImpl: path.PlatformPath
): string {
  const isWin32 = pathImpl === path.win32
  const trimmedTarget = relativeTarget.replace(isWin32 ? /^(?:\.[/\\]+)+/ : /^(?:\.\/)+/, '')
  if (!trimmedTarget || trimmedTarget === '.') return baseDir
  if (!baseDir || baseDir === '.') return trimmedTarget
  const hasTrailingSep = isWin32
    ? (baseDir.endsWith('/') || baseDir.endsWith('\\'))
    : baseDir.endsWith('/')
  return hasTrailingSep ? `${baseDir}${trimmedTarget}` : `${baseDir}${pathImpl.sep}${trimmedTarget}`
}

function resolveWin32Target (
  baseDir: string,
  targetPath: string
): { done: string } | { done?: undefined, baseDir: string, relativeTarget: string } {
  if (/^[/\\](?![/\\])/.test(targetPath)) {
    const baseDevice = path.win32.parse(baseDir).root.replace(/[/\\]$/, '')
    return { done: baseDevice !== '' ? `${baseDevice}${targetPath}` : targetPath }
  }
  if (path.win32.isAbsolute(targetPath)) {
    return { done: targetPath }
  }
  const targetDrive = targetPath.match(/^[A-Z]:/i)?.[0]
  if (targetDrive == null) {
    return { baseDir, relativeTarget: targetPath }
  }
  const baseDrive = baseDir.match(/^[A-Z]:/i)?.[0]
  const effectiveBaseDir = baseDrive?.toLowerCase() === targetDrive.toLowerCase()
    ? baseDir
    : path.win32.resolve(targetDrive)
  return { baseDir: effectiveBaseDir, relativeTarget: targetPath.slice(targetDrive.length) }
}
