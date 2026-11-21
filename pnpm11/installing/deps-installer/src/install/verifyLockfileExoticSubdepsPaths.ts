import fs from 'node:fs'
import path from 'node:path'

import { isError } from '@pnpm/error'
import type { ProjectManifest } from '@pnpm/types'
import { readExactProjectManifestSync } from '@pnpm/workspace.project-manifest-reader'

const WIN_DRIVE_PREFIX_RE = /^([a-z]):[/\\]/i

function trimSlashes (value: string, { leading }: { leading: boolean }): string {
  let start = 0
  let end = value.length
  if (leading) {
    while (start < end && value.charCodeAt(start) === 47) start += 1
  }
  while (end > start && value.charCodeAt(end - 1) === 47) end -= 1
  return value.slice(start, end)
}

export function normalizePosixRelPath (rawPath: string): string {
  const driveMatch = WIN_DRIVE_PREFIX_RE.exec(rawPath)
  const slashPath = rawPath.replaceAll('\\', '/')
  if (driveMatch != null) {
    const drive = driveMatch[1].toUpperCase()
    const rest = trimSlashes(path.posix.normalize(`/${slashPath.slice(3)}`), { leading: true })
    return `${drive}:/${rest}`
  }
  const normalized = path.posix.normalize(slashPath)
  return normalized === '/' ? '/' : (trimSlashes(normalized, { leading: false }) || '.')
}

export function joinPosixRelPath (base: string, rel: string): string {
  const slashRel = rel.replaceAll('\\', '/')
  if (slashRel.startsWith('/') || WIN_DRIVE_PREFIX_RE.test(rel)) return normalizePosixRelPath(slashRel)
  if (base === '.' || base === '') return normalizePosixRelPath(slashRel)
  const slashBase = trimSlashes(base.replaceAll('\\', '/'), { leading: false })
  return normalizePosixRelPath(`${slashBase}/${slashRel}`)
}

export function isCanonicalImporterId (id: string): boolean {
  return id !== '' && !id.includes('\\') && normalizePosixRelPath(id) === id
}

export function commonLexicalAncestor (leftDir: string, rightDir: string): string | undefined {
  const leftResolved = path.resolve(leftDir)
  const rightResolved = path.resolve(rightDir)
  const leftParsed = path.parse(leftResolved)
  const rightParsed = path.parse(rightResolved)
  if (leftParsed.root.toLowerCase() !== rightParsed.root.toLowerCase()) return undefined
  const leftParts = leftResolved.slice(leftParsed.root.length).split(path.sep).filter(Boolean)
  const rightParts = rightResolved.slice(rightParsed.root.length).split(path.sep).filter(Boolean)
  const shared: string[] = []
  const limit = Math.min(leftParts.length, rightParts.length)
  for (let idx = 0; idx < limit; idx += 1) {
    if (leftParts[idx] !== rightParts[idx]) break
    shared.push(leftParts[idx])
  }
  return path.join(leftParsed.root, ...shared)
}

export function isRealWorkspaceTarget (projectRoot: string, candidate: string): boolean {
  const commonBase = commonLexicalAncestor(projectRoot, candidate)
  const segments = commonBase != null ? confinedSubpathSegments(commonBase, candidate, { allowSameDir: true }) : undefined
  if (commonBase == null || segments == null || !isRealDirectory(candidate)) return false
  const realBase = realPathOrUndefined(commonBase)
  const realCandidate = realBase != null ? realPathOrUndefined(candidate) : undefined
  if (realBase == null || realCandidate == null) return false
  return realCandidate === path.join(realBase, ...segments)
}

export function isOwnPublishDir (rootDir: string, publishDir: string): boolean {
  const segments = confinedSubpathSegments(rootDir, publishDir, { allowSameDir: false })
  const realRootDir = segments != null ? realPathOrUndefined(rootDir) : undefined
  if (segments == null || realRootDir == null) return false
  let current = rootDir
  let expectedReal = realRootDir
  for (const segment of segments) {
    current = path.join(current, segment)
    expectedReal = path.join(expectedReal, segment)
    const verdict = verifyPublishSegment(current, expectedReal)
    if (verdict != null) return verdict
  }
  return true
}

const PROJECT_MANIFEST_FILENAMES = ['package.json', 'package.json5', 'package.yaml'] as const

type ExactManifestReadResult =
  | { status: 'found', manifest: ProjectManifest }
  | { status: 'absent' }
  | { status: 'blocked' }

export function readManifestIfExists (dir: string): ProjectManifest | undefined {
  for (const fileName of PROJECT_MANIFEST_FILENAMES) {
    const result = tryReadExactManifest(path.join(dir, fileName))
    if (result.status === 'found') return result.manifest
    if (result.status === 'blocked') return undefined
  }
  return undefined
}

function confinedSubpathSegments (
  baseDir: string,
  targetDir: string,
  { allowSameDir }: { allowSameDir: boolean }
): string[] | undefined {
  const relativePath = path.relative(baseDir, targetDir)
  if (relativePath === '') return allowSameDir ? [] : undefined
  if (path.isAbsolute(relativePath)) return undefined
  const segments = relativePath.split(path.sep)
  const invalid = segments[0] === '..' ||
    segments.some((seg) => seg === '' || seg === '.' || seg === '..' || seg.toLowerCase() === 'node_modules')
  return invalid ? undefined : segments
}

function isRealDirectory (dir: string): boolean {
  try {
    const stat = fs.lstatSync(dir)
    return !stat.isSymbolicLink() && stat.isDirectory()
  } catch (err: unknown) {
    if (isFsMissingError(err)) return false
    throw err
  }
}

function verifyPublishSegment (current: string, expectedReal: string): boolean | undefined {
  let stat: fs.Stats
  try {
    stat = fs.lstatSync(current)
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && err.code === 'ENOENT') return true
    if (isFsMissingError(err)) return false
    throw err
  }
  if (stat.isSymbolicLink() || !stat.isDirectory()) return false
  return realPathOrUndefined(current) === expectedReal ? undefined : false
}

function realPathOrUndefined (dir: string): string | undefined {
  try {
    return fs.realpathSync(dir)
  } catch (err: unknown) {
    if (isFsMissingError(err)) return undefined
    throw err
  }
}

function tryReadExactManifest (manifestPath: string): ExactManifestReadResult {
  try {
    return { status: 'found', manifest: readExactProjectManifestSync(manifestPath).manifest }
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && err.code === 'ENOENT') return { status: 'absent' }
    if (isFsMissingError(err) || isManifestParseError(err)) return { status: 'blocked' }
    throw err
  }
}

function isManifestParseError (err: unknown): boolean {
  return isError(err) &&
    'code' in err &&
    (err.code === 'ERR_PNPM_JSON_PARSE' || err.code === 'ERR_PNPM_JSON5_PARSE' || err.code === 'ERR_PNPM_YAML_PARSE')
}

function isFsMissingError (err: unknown): boolean {
  return isError(err) &&
    'code' in err &&
    (err.code === 'ENOENT' || err.code === 'ENOTDIR' || err.code === 'EISDIR' || err.code === 'EACCES' || err.code === 'EPERM')
}
