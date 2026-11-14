import fs from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import { PnpmError } from '@pnpm/error'

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

export async function canonicalizePackDestination (
  destDir: string,
  cache?: Map<string, Promise<string>>
): Promise<string> {
  let pending = cache?.get(destDir)
  if (pending == null) {
    pending = resolveCanonicalDestination(destDir)
    cache?.set(destDir, pending)
  }
  try {
    return await pending
  } catch (err: unknown) {
    cache?.delete(destDir)
    throw err
  }
}

async function resolveCanonicalDestination (destDir: string): Promise<string> {
  try {
    return await fs.promises.realpath(destDir)
  } catch (err: unknown) {
    if (!isMissingPathError(err)) throw err
    return resolveMissingDestination(resolveRelativeTarget(process.cwd(), destDir))
  }
}

async function resolveMissingDestination (anchoredDir: string): Promise<string> {
  const { root } = path.parse(anchoredDir)
  const segments = splitPathSegments(anchoredDir.slice(root.length))
    .filter((segment) => segment !== '' && segment !== '.')
  return walkDestinationSegments(segments, 0, { current: root || '.', missingTail: false })
}

function splitPathSegments (relativePath: string): string[] {
  return relativePath.split(process.platform === 'win32' ? /[/\\]+/ : /\/+/)
}

async function walkDestinationSegments (
  segments: string[],
  index: number,
  state: { current: string, missingTail: boolean }
): Promise<string> {
  if (index >= segments.length) return state.current
  const nextState = await stepDestinationSegment(state.current, segments[index], state.missingTail)
  return walkDestinationSegments(segments, index + 1, nextState)
}

async function stepDestinationSegment (
  current: string,
  segment: string,
  missingTail: boolean
): Promise<{ current: string, missingTail: boolean }> {
  if (segment === '..') {
    const parent = path.dirname(current)
    return missingTail ? tryRealpath(parent) : { current: parent, missingTail: false }
  }
  const next = path.join(current, segment)
  return missingTail ? { current: next, missingTail: true } : tryRealpath(next)
}

async function tryRealpath (candidate: string): Promise<{ current: string, missingTail: boolean }> {
  try {
    return { current: await fs.promises.realpath(candidate), missingTail: false }
  } catch (err: unknown) {
    if (!isMissingPathError(err)) throw err
    return { current: candidate, missingTail: true }
  }
}

function isMissingPathError (err: unknown): boolean {
  return util.types.isNativeError(err) && 'code' in err && (err.code === 'ENOENT' || err.code === 'ENOTDIR')
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
