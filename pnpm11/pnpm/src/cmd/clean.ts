import { promises as fs, type Stats } from 'node:fs'
import path from 'node:path'

import { docsUrl } from '@pnpm/cli.utils'
import { isError } from '@pnpm/error'
import { findWorkspaceProjectsNoCheck } from '@pnpm/workspace.projects-reader'
import { rimraf } from '@zkochan/rimraf'
import { isSubdir, strict as isStrictSubdir } from 'is-subdir'
import { renderHelp } from 'render-help'

export const commandNames = ['clean', 'purge']

export const overridableByScript = true

export const rcOptionsTypes = (): Record<string, unknown> => ({})

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    lockfile: Boolean,
  }
}

export const shorthands: Record<string, string> = {
  l: '--lockfile',
}

export function help (): string {
  return renderHelp({
    aliases: ['purge'],
    description: 'Safely remove node_modules directories from all workspace projects. \
Uses Node.js to remove directories, which correctly handles NTFS junctions on Windows \
without following them into their targets. \
If the current project has a "clean" (or "purge") script in package.json, \
the script is executed instead of the built-in command.',
    descriptionLists: [
      {
        title: 'Options',
        list: [
          {
            description: 'Also remove pnpm-lock.yaml files',
            name: '--lockfile',
            shortAlias: '-l',
          },
        ],
      },
    ],
    url: docsUrl('clean'),
    usages: ['pnpm clean [--lockfile]'],
  })
}

export interface CleanOptions {
  dir: string
  modulesDir?: string
  virtualStoreDir?: string
  workspaceDir?: string
  workspacePackagePatterns?: string[]
}

export async function handler (
  opts: CleanOptions & {
    cliOptions?: {
      lockfile?: boolean
    }
  }
): Promise<void> {
  await cleanProjects({ ...opts, removeLockfile: opts.cliOptions?.lockfile === true })
}

interface CleanProjectContext {
  cwd: string
  modulesDir: string
  realRootDir: string
  removeLockfile: boolean
}

export async function cleanProjects (opts: CleanOptions & { removeLockfile: boolean }): Promise<void> {
  const modulesDir = opts.modulesDir ?? 'node_modules'
  const rootDir = opts.workspaceDir ?? opts.dir
  const realRootDir = await tryRealpath(rootDir)
  if (realRootDir == null) return
  const cwd = await tryRealpath(process.cwd()) ?? process.cwd()
  const cleanOpts: CleanProjectContext = {
    cwd,
    modulesDir,
    realRootDir,
    removeLockfile: opts.removeLockfile,
  }
  const dirs = await getProjectDirs(opts)
  await Promise.all(dirs.map(async (dir) => cleanProjectDir(cleanOpts, dir)))
  if (opts.virtualStoreDir) {
    await removeExternalVirtualStore({
      cwd,
      modulesDir,
      rootDir,
      virtualStoreDir: opts.virtualStoreDir,
    })
  }
}

async function cleanProjectDir (opts: CleanProjectContext, dir: string): Promise<void> {
  const fullModulesDir = await resolveProjectModulesDir({
    modulesDir: opts.modulesDir,
    projectDir: dir,
    realRootDir: opts.realRootDir,
  })
  if (fullModulesDir != null) {
    await cleanModulesDir(opts.cwd, fullModulesDir)
  }
  if (opts.removeLockfile) {
    await removeProjectLockfile(opts, dir)
  }
}

async function resolveProjectModulesDir (
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

const PNPM_HIDDEN_ENTRIES = new Set(['.bin', '.modules.yaml', '.pnpm', '.pnpm-workspace-state-v1.json'])

async function cleanModulesDir (cwd: string, modulesDir: string): Promise<void> {
  const stat = await lstatIfExists(modulesDir)
  if (stat == null) return
  if (stat.isSymbolicLink()) {
    printRemoving(cwd, modulesDir)
    await rimraf(modulesDir)
    return
  }
  if (!stat.isDirectory()) return
  const entries = await removableModulesEntries(modulesDir)
  if (entries.length === 0) return
  printRemoving(cwd, modulesDir)
  await Promise.all(entries.map(async (entry) => rimraf(entry)))
}

async function removableModulesEntries (modulesDir: string): Promise<string[]> {
  let items: string[]
  try {
    items = await fs.readdir(modulesDir)
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && err.code === 'ENOENT') return []
    throw err
  }
  return items
    .filter((item) => item[0] !== '.' || PNPM_HIDDEN_ENTRIES.has(item))
    .map((item) => path.join(modulesDir, item))
}

async function removeProjectLockfile (opts: CleanProjectContext, dir: string): Promise<void> {
  const lockfilePath = await resolveInside(dir, 'pnpm-lock.yaml')
  if (lockfilePath == null || !isStrictSubdir(opts.realRootDir, lockfilePath)) return
  if (await lstatIfExists(lockfilePath) == null) return
  printRemoving(opts.cwd, lockfilePath)
  await rimraf(lockfilePath)
}

async function removeExternalVirtualStore (
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

async function resolveInside (rootDir: string, targetSpec: string): Promise<string | undefined> {
  const candidatePath = normalizeTargetSpec(rootDir, targetSpec)
  const parsed = path.parse(candidatePath)
  if (parsed.base === '' || parsed.base === '.' || parsed.base === '..') {
    return undefined
  }
  const [realRoot, realParent] = await Promise.all([
    tryRealpath(rootDir),
    tryRealpath(parsed.dir),
  ])
  if (realRoot == null || realParent == null) {
    return undefined
  }
  const resolved = path.join(realParent, parsed.base)
  return isStrictSubdir(realRoot, resolved) ? resolved : undefined
}

function normalizeTargetSpec (rootDir: string, targetSpec: string): string {
  const nativeSpec = path.sep === '\\' ? targetSpec.replaceAll('/', '\\') : targetSpec
  if (path.parse(nativeSpec).root === '') {
    return `${rootDir}${path.sep}${nativeSpec}`
  }
  return path.isAbsolute(nativeSpec) ? nativeSpec : path.resolve(nativeSpec)
}

async function tryRealpath (targetPath: string): Promise<string | undefined> {
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

async function getProjectDirs (
  opts: {
    dir: string
    modulesDir?: string
    modulesDirsByProjectName?: Record<string, string>
    workspaceDir?: string
    workspacePackagePatterns?: string[]
  }
): Promise<string[]> {
  if (!opts.workspaceDir) {
    return [opts.dir]
  }
  const pkgs = await findWorkspaceProjectsNoCheck(opts.workspaceDir, {
    patterns: opts.workspacePackagePatterns,
    modulesDir: opts.modulesDir,
    modulesDirsByProjectName: opts.modulesDirsByProjectName,
  })
  return pkgs.map((pkg) => pkg.rootDir)
}
