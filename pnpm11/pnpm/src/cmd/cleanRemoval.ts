import { promises as fs, type Stats } from 'node:fs'
import path, { type ParsedPath } from 'node:path'

import { isError } from '@pnpm/error'
import { rimraf } from '@zkochan/rimraf'
import { isSubdir, strict as isStrictSubdir } from 'is-subdir'

const PNPM_HIDDEN_ENTRIES = new Set(['.bin', '.modules.yaml', '.pnpm', '.pnpm-workspace-state-v1.json'])
const PNPM_NARROWED_SHARED_HIDDEN_ENTRIES = new Set(['.bin', '.pnpm-workspace-state-v1.json'])

export async function resolveProjectModulesDir (
  opts: {
    modulesDir: string
    projectDir: string
    realRootDir: string
  }
): Promise<string | undefined> {
  const resolved = await resolveInside(opts.projectDir, opts.modulesDir)
  if (resolved == null || !isStrictSubdir(opts.realRootDir, resolved)) {
    return undefined
  }
  return resolved
}

export async function resolveConfiguredLockfileDir (
  rawSpec: string,
  rootDir: string,
  realRootDir: string
): Promise<string | undefined> {
  const candidate = await resolveInside(rootDir, rawSpec)
  if (candidate == null) return undefined
  const realLockfileDir = await tryRealpath(candidate)
  return realLockfileDir != null && isStrictSubdir(realRootDir, realLockfileDir)
    ? realLockfileDir
    : undefined
}

export async function cleanModulesDir (cwd: string, modulesDir: string, preserveShared: boolean): Promise<void> {
  const stat = await lstatIfExists(modulesDir)
  if (stat == null) return
  if (stat.isSymbolicLink()) {
    if (preserveShared) return
    printRemoving(cwd, modulesDir)
    await rimraf(modulesDir)
    return
  }
  if (!stat.isDirectory()) return
  const entries = await removableModulesEntries(modulesDir, preserveShared)
  if (entries.length === 0) return
  printRemoving(cwd, modulesDir)
  await Promise.all(entries.map(async (entry) => rimraf(entry)))
}

async function removableModulesEntries (modulesDir: string, preserveShared: boolean): Promise<string[]> {
  let items: string[]
  try {
    items = await fs.readdir(modulesDir)
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && err.code === 'ENOENT') return []
    throw err
  }
  const hiddenEntries = preserveShared ? PNPM_NARROWED_SHARED_HIDDEN_ENTRIES : PNPM_HIDDEN_ENTRIES
  return items
    .filter((item) => item[0] !== '.' || hiddenEntries.has(item))
    .map((item) => path.join(modulesDir, item))
}

export async function removeLockfilePath (cwd: string, lockfilePath: string): Promise<void> {
  const stat = await lstatIfExists(lockfilePath)
  if (stat == null || (!stat.isSymbolicLink() && stat.isDirectory())) return
  printRemoving(cwd, lockfilePath)
  await rimraf(lockfilePath)
}

export async function invalidateWorkspaceState (realRootDir: string, sharedModulesDir: string): Promise<void> {
  const statePath = await resolveInside(sharedModulesDir, '.pnpm-workspace-state-v1.json')
  if (statePath == null || !isStrictSubdir(realRootDir, statePath)) return
  const stat = await lstatIfExists(statePath)
  if (stat == null || (!stat.isSymbolicLink() && stat.isDirectory())) return
  await rimraf(statePath)
}

export async function removeExternalVirtualStore (
  opts: {
    cwd: string
    modulesDir: string
    rootDir: string
    virtualStoreDir: string
  }
): Promise<void> {
  const virtualStoreInProject = await resolveInside(opts.rootDir, opts.virtualStoreDir)
  if (virtualStoreInProject == null) return
  const rootModulesDir = await resolveInside(opts.rootDir, opts.modulesDir)
  if (rootModulesDir != null && isSubdir(rootModulesDir, virtualStoreInProject)) return
  if (await lstatIfExists(virtualStoreInProject) == null) return
  printRemoving(opts.cwd, virtualStoreInProject)
  await rimraf(virtualStoreInProject)
}

export async function resolveInside (rootDir: string, targetSpec: string): Promise<string | undefined> {
  const realRoot = await tryRealpath(rootDir)
  if (realRoot == null) return undefined
  const candidatePath = normalizeTargetSpec(realRoot, targetSpec)
  if (candidatePath == null) return undefined
  const parsed = path.parse(candidatePath)
  if (parsed.base.replace(/[. ]+$/u, '') === '') return undefined
  const realParent = await resolveParentDir(realRoot, parsed)
  if (realParent == null) return undefined
  const resolved = path.join(realParent, parsed.base)
  return isStrictSubdir(realRoot, resolved) ? resolved : undefined
}

async function resolveParentDir (realRoot: string, parsed: ParsedPath): Promise<string | undefined> {
  if (parsed.dir === realRoot) return realRoot
  if (!parsed.dir.split(path.sep).some(needsStepwiseResolution)) {
    return tryRealpath(parsed.dir)
  }
  return resolveParentSegments(realRoot, parsed)
}

function needsStepwiseResolution (segment: string): boolean {
  return segment === '..' || (path.sep === '\\' && segment !== '' && segment.replace(/[. ]+$/u, '') === '')
}

async function resolveParentSegments (realRoot: string, parsed: ParsedPath): Promise<string | undefined> {
  const rootPrefix = appendPathSep(realRoot)
  const underRealRoot = parsed.dir.startsWith(rootPrefix)
  const anchor = underRealRoot ? realRoot : await tryRealpath(parsed.root)
  if (anchor == null) return undefined
  const tail = underRealRoot ? parsed.dir.slice(rootPrefix.length) : parsed.dir.slice(parsed.root.length)
  const segments = tail.split(path.sep).filter((segment) => segment !== '' && segment !== '.')
  return segments.reduce<Promise<string | undefined>>(
    async (currentPromise, segment) => stepParentSegment(await currentPromise, segment),
    Promise.resolve(anchor)
  )
}

async function stepParentSegment (current: string | undefined, segment: string): Promise<string | undefined> {
  if (current == null) return undefined
  return segment === '..' ? path.dirname(current) : tryRealpath(path.join(current, segment))
}

function normalizeTargetSpec (realRoot: string, targetSpec: string): string | undefined {
  const nativeSpec = path.sep === '\\' ? targetSpec.replaceAll('/', '\\') : targetSpec
  if (path.parse(nativeSpec).root === '') {
    return `${appendPathSep(realRoot)}${nativeSpec}`
  }
  return path.isAbsolute(nativeSpec) ? nativeSpec : undefined
}

function appendPathSep (dirPath: string): string {
  return dirPath.endsWith(path.sep) ? dirPath : `${dirPath}${path.sep}`
}

export async function tryRealpath (targetPath: string): Promise<string | undefined> {
  try {
    return await fs.realpath(targetPath)
  } catch (err: unknown) {
    if (isExpectedResolutionError(err)) return undefined
    throw err
  }
}

async function lstatIfExists (targetPath: string): Promise<Stats | undefined> {
  try {
    return await fs.lstat(targetPath)
  } catch (err: unknown) {
    if (isExpectedResolutionError(err)) return undefined
    throw err
  }
}

function isExpectedResolutionError (err: unknown): boolean {
  return isError(err) &&
    'code' in err &&
    (err.code === 'ENOENT' || err.code === 'ENOTDIR' || err.code === 'ELOOP')
}

function printRemoving (cwd: string, dir: string): void {
  process.stdout.write(`Removing ${path.relative(cwd, dir) || '.'}\n`)
}
