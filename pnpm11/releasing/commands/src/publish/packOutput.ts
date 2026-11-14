import path from 'node:path'

import { PnpmError } from '@pnpm/error'

import { normalizePackageName } from '../tarball/safeTarballFilename.js'

export function assertValidTarballVersion (version: string, reportedVersion: string = version): void {
  if (!version || version === '.' || version === '..' || version.includes('/') || version.includes('\\')) {
    throw new PnpmError('INVALID_PACKAGE_VERSION', `Invalid package version "${reportedVersion}".`)
  }
}

export function stripBuildMetadata (version: string): string {
  const plusIndex = version.indexOf('+')
  return plusIndex === -1 ? version : version.slice(0, plusIndex)
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
  const baseDrive = /^[A-Z]:/i.exec(baseDir)?.[0]
  if (/^[/\\](?![/\\])/.test(targetPath)) {
    return { done: baseDrive != null ? `${baseDrive}${targetPath}` : targetPath }
  }
  if (path.win32.isAbsolute(targetPath)) {
    return { done: targetPath }
  }
  const targetDrive = /^[A-Z]:/i.exec(targetPath)?.[0]
  if (targetDrive == null) {
    return { baseDir, relativeTarget: targetPath }
  }
  const effectiveBaseDir = baseDrive?.toLowerCase() === targetDrive.toLowerCase()
    ? baseDir
    : path.win32.resolve(targetDrive)
  return { baseDir: effectiveBaseDir, relativeTarget: targetPath.slice(targetDrive.length) }
}
