import { promises as fs, type Stats } from 'node:fs'
import path, { type ParsedPath } from 'node:path'

import { isError } from '@pnpm/error'
import { rimraf } from '@zkochan/rimraf'
import { isSubdir, strict as isStrictSubdir } from 'is-subdir'

const PNPM_HIDDEN_ENTRIES = new Set(['.bin', '.modules.yaml', '.pnpm', '.pnpm-workspace-state-v1.json'])
const PNPM_NARROWED_SHARED_HIDDEN_ENTRIES = new Set(['.bin', '.pnpm-workspace-state-v1.json'])

export type ModulesCleanMode =
  | { kind: 'all', preservedEntry?: string }
  | { kind: 'preserve-shared-virtual-store', preservedEntry?: string }

export interface PlannedModulesTarget {
  dir: string
  mode: ModulesCleanMode
}

export function shouldRemoveModulesEntry (name: string, mode: ModulesCleanMode): boolean {
  switch (mode.kind) {
    case 'all':
      return name !== mode.preservedEntry &&
        (name[0] !== '.' || PNPM_HIDDEN_ENTRIES.has(name))
    case 'preserve-shared-virtual-store':
      return name !== mode.preservedEntry &&
        (name[0] !== '.' || PNPM_NARROWED_SHARED_HIDDEN_ENTRIES.has(name))
  }
}

export async function removesEntryInModulesDir (
  modulesDir: string,
  mode: ModulesCleanMode,
  entryName: string
): Promise<boolean> {
  const stat = await lstatIfExists(modulesDir)
  if (stat?.isSymbolicLink()) {
    switch (mode.kind) {
      case 'all':
        return mode.preservedEntry == null
      case 'preserve-shared-virtual-store':
        return false
    }
  }
  return shouldRemoveModulesEntry(entryName, mode)
}

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

export async function resolveSharedAnchorDir (
  ctx: {
    rawLockfileSpec: string | undefined
    realRootDir: string
    resolvedLockfileDir: string | undefined
    rootDir: string
  }
): Promise<string | undefined> {
  if (ctx.rawLockfileSpec == null) return ctx.rootDir
  if (ctx.resolvedLockfileDir != null) return ctx.resolvedLockfileDir
  const candidate = await resolveRealPathFrom(ctx.rootDir, ctx.rawLockfileSpec)
  return candidate === ctx.realRootDir ? ctx.rootDir : undefined
}

export function isCleanableModulesDir (
  resolved: string | undefined,
  seen: Set<string>,
  opts: { ownerDir?: string, protectedMemberDirs: string[] }
): resolved is string {
  return resolved != null &&
    !seen.has(resolved) &&
    !opts.protectedMemberDirs.some((memberDir) => (
      isSubdir(resolved, memberDir) ||
      ((opts.ownerDir == null || !isSubdir(memberDir, opts.ownerDir)) && isSubdir(memberDir, resolved))
    ))
}

export async function resolveSharedVirtualStoreDir (
  opts: {
    anchorDir: string | undefined
    enableGlobalVirtualStore?: boolean
    realRootDir: string
    sharedModulesDir: string | undefined
    virtualStoreDir: string | undefined
  }
): Promise<string | undefined> {
  if (opts.enableGlobalVirtualStore) return undefined
  if (opts.virtualStoreDir) {
    if (opts.anchorDir == null) return undefined
    return resolveInside(opts.realRootDir, path.resolve(opts.anchorDir, opts.virtualStoreDir))
  }
  return opts.sharedModulesDir != null
    ? resolveInside(opts.realRootDir, path.join(opts.sharedModulesDir, '.pnpm'))
    : undefined
}

export function preservedEntryForTarget (
  targetModulesDir: string,
  sharedVirtualStoreDir: string | undefined
): string | undefined {
  if (sharedVirtualStoreDir == null || !isStrictSubdir(targetModulesDir, sharedVirtualStoreDir)) {
    return undefined
  }
  const firstSegment = path.relative(targetModulesDir, sharedVirtualStoreDir).split(path.sep)[0]
  return firstSegment || undefined
}

export async function dirsIncludeRoot (dirs: string[], realRootDir: string): Promise<boolean> {
  const realDirs = await Promise.all(dirs.map(async (dir) => tryRealpath(dir)))
  return realDirs.includes(realRootDir)
}

export async function collectNonRootRealDirs (dirs: string[], realRootDir: string): Promise<string[]> {
  const realDirs = await Promise.all(dirs.map(async (dir) => tryRealpath(dir)))
  return realDirs.filter((dir): dir is string => dir != null && dir !== realRootDir)
}

export async function cleanModulesDir (cwd: string, modulesDir: string, mode: ModulesCleanMode): Promise<void> {
  const stat = await lstatIfExists(modulesDir)
  if (stat == null) return
  if (stat.isSymbolicLink()) {
    if (mode.kind === 'preserve-shared-virtual-store' || mode.preservedEntry != null) return
    printRemoving(cwd, modulesDir)
    await rimraf(modulesDir)
    return
  }
  if (!stat.isDirectory()) return
  const entries = await removableModulesEntries(modulesDir, mode)
  if (entries.length === 0) return
  printRemoving(cwd, modulesDir)
  await Promise.all(entries.map(async (entry) => rimraf(entry)))
}

async function removableModulesEntries (modulesDir: string, mode: ModulesCleanMode): Promise<string[]> {
  let items: string[]
  try {
    items = await fs.readdir(modulesDir)
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && err.code === 'ENOENT') return []
    throw err
  }
  return items
    .filter((item) => shouldRemoveModulesEntry(item, mode))
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

export async function resolveExternalVirtualStoreTarget (
  opts: {
    modulesDir: string
    protectedMemberDirs: string[]
    rootDir: string
    virtualStoreDir: string
  }
): Promise<string | undefined> {
  const virtualStoreInProject = await resolveInside(opts.rootDir, opts.virtualStoreDir)
  if (!isCleanableModulesDir(virtualStoreInProject, new Set(), { protectedMemberDirs: opts.protectedMemberDirs })) {
    return undefined
  }
  const rootModulesDir = await resolveInside(opts.rootDir, opts.modulesDir)
  if (rootModulesDir != null && isSubdir(rootModulesDir, virtualStoreInProject)) return undefined
  return virtualStoreInProject
}

export async function removeExternalVirtualStore (
  opts: {
    cwd: string
    externalVirtualStoreDir: string | undefined
  }
): Promise<void> {
  if (opts.externalVirtualStoreDir == null) return
  if (await lstatIfExists(opts.externalVirtualStoreDir) == null) return
  printRemoving(opts.cwd, opts.externalVirtualStoreDir)
  await rimraf(opts.externalVirtualStoreDir)
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

export async function resolveRealPathFrom (rootDir: string, targetSpec: string): Promise<string | undefined> {
  const realRoot = await tryRealpath(rootDir)
  if (realRoot == null) return undefined
  const candidatePath = normalizeTargetSpec(realRoot, targetSpec)
  if (candidatePath == null) return undefined
  const parsed = path.parse(candidatePath)
  const segments = candidatePath.slice(parsed.root.length).split(path.sep).filter((segment) => segment !== '' && segment !== '.')
  return reduceSegments(tryRealpath(parsed.root), segments)
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
  const anchorPromise = underRealRoot ? Promise.resolve(realRoot) : tryRealpath(parsed.root)
  const tail = underRealRoot ? parsed.dir.slice(rootPrefix.length) : parsed.dir.slice(parsed.root.length)
  const segments = tail.split(path.sep).filter((segment) => segment !== '' && segment !== '.')
  return reduceSegments(anchorPromise, segments)
}

async function reduceSegments (
  anchorPromise: Promise<string | undefined>,
  segments: string[]
): Promise<string | undefined> {
  return segments.reduce<Promise<string | undefined>>(
    async (currentPromise, segment) => stepParentSegment(await currentPromise, segment),
    anchorPromise
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
