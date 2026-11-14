import { promises as fs } from 'node:fs'
import path from 'node:path'

import { isError } from '@pnpm/error'
import { readModulesManifest } from '@pnpm/installing.modules-yaml'
import { findWorkspaceProjectsNoCheck, type Project } from '@pnpm/workspace.projects-reader'

import {
  escapeGlobPath,
  literalPatternSubpaths,
  pathsMatchOnHost,
  patternsEnteringRootModulesDir,
} from './cleanPatternReachability.js'

export interface CleanWorkspaceDiscoveryOptions {
  hasPerProjectModulesDirs: boolean
  modulesDir?: string
  modulesDirsByProjectName?: Record<string, string>
  patterns?: string[]
}

export async function discoverWorkspacePackagesForClean (
  workspaceDir: string,
  opts: CleanWorkspaceDiscoveryOptions
): Promise<Project[]> {
  const relModules = relativeRootModulesPosix(workspaceDir, opts.modulesDir)
  if (relModules == null) {
    return findWorkspaceProjectsNoCheck(workspaceDir, {
      patterns: opts.patterns,
      modulesDir: opts.modulesDir,
      modulesDirsByProjectName: opts.modulesDirsByProjectName,
    })
  }
  const basePatterns = [...(opts.patterns ?? ['.', '**']), `!${escapeGlobPath(relModules)}/**`]
  const projects = await findWorkspaceProjectsNoCheck(workspaceDir, {
    patterns: basePatterns,
    modulesDir: opts.modulesDir,
    modulesDirsByProjectName: opts.modulesDirsByProjectName,
  })
  const customLeaves = collectCustomRelativeModulesLeaves(opts)
  const hoistedSubpaths = await readHoistedSubpathsUnderRootModules(workspaceDir, relModules)
  const recoveryPatterns = patternsEnteringRootModulesDir({
    customLeaves,
    hoistedSubpaths,
    patterns: opts.patterns,
    relModules,
  })
  if (recoveryPatterns == null) {
    return projects
  }
  const candidates = await findWorkspaceProjectsNoCheck(workspaceDir, { patterns: recoveryPatterns })
  return mergeRecoveryCandidates({
    found: candidates,
    known: projects,
    opts,
    workspaceDir,
  })
}

function relativeRootModulesPosix (workspaceDir: string, modulesDir: string | undefined): string | undefined {
  if (modulesDir == null) return undefined
  const rel = path.isAbsolute(modulesDir)
    ? path.relative(workspaceDir, modulesDir)
    : path.relative('.', modulesDir)
  return normalizeCustomRelativePosix(rel)
}

function collectCustomRelativeModulesLeaves (opts: CleanWorkspaceDiscoveryOptions): string[] {
  const leaves = new Set<string>()
  for (const raw of [opts.modulesDir, ...Object.values(opts.modulesDirsByProjectName ?? {})]) {
    if (raw == null || path.isAbsolute(raw)) continue
    const posix = normalizeCustomRelativePosix(path.relative('.', raw))
    if (posix != null) {
      leaves.add(posix)
    }
  }
  return Array.from(leaves)
}

function normalizeCustomRelativePosix (rel: string): string | undefined {
  if (rel === '' || rel === '..' || rel.startsWith(`..${path.sep}`) || path.isAbsolute(rel)) {
    return undefined
  }
  const posix = rel.split(path.sep).join('/')
  if (pathsMatchOnHost(posix, 'node_modules') || pathsMatchOnHost(posix, 'bower_components')) {
    return undefined
  }
  return posix
}

async function readHoistedSubpathsUnderRootModules (
  workspaceDir: string,
  relModules: string
): Promise<string[]> {
  const resolvedWorkspaceDir = path.resolve(workspaceDir)
  const resolvedRootModulesDir = path.resolve(resolvedWorkspaceDir, relModules)
  let hoistedLocations: Record<string, string[]> | undefined
  try {
    const manifest = await readModulesManifest(resolvedRootModulesDir)
    hoistedLocations = manifest?.hoistedLocations
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && err.code === 'ENOTDIR') {
      return []
    }
    throw err
  }
  if (hoistedLocations == null) return []
  const subpaths = new Set<string>()
  for (const loc of Object.values(hoistedLocations).flat()) {
    const candidate = path.resolve(resolvedWorkspaceDir, loc)
    if (!candidate.startsWith(`${resolvedRootModulesDir}${path.sep}`)) continue
    const rel = path.relative(resolvedWorkspaceDir, candidate)
    const posix = normalizeCustomRelativePosix(rel)
    if (posix != null) subpaths.add(posix)
  }
  return Array.from(subpaths)
}

interface MergeRecoveryCandidatesOptions {
  found: Project[]
  known: Project[]
  opts: CleanWorkspaceDiscoveryOptions
  workspaceDir: string
}

async function mergeRecoveryCandidates (
  { found, known, opts, workspaceDir }: MergeRecoveryCandidatesOptions
): Promise<Project[]> {
  const resolvedWorkspaceDir = path.resolve(workspaceDir)
  const knownDirs = new Set(known.map((project) => project.rootDir))
  const ownedModulesDirs = buildNonRootOwnedModulesDirs(resolvedWorkspaceDir, [...known, ...found], opts)
  const checks = await Promise.all(found.map(async (candidate) => ({
    candidate,
    keep: !knownDirs.has(candidate.rootDir) &&
      !isInsideOwnedModulesDir(candidate.rootDir, ownedModulesDirs) &&
      !await hasSymlinkComponentUnder(resolvedWorkspaceDir, candidate.rootDir),
  })))
  return [...known, ...checks.filter(({ keep }) => keep).map(({ candidate }) => candidate)]
}

function buildNonRootOwnedModulesDirs (
  resolvedWorkspaceDir: string,
  projects: Project[],
  opts: CleanWorkspaceDiscoveryOptions
): Map<string, string> {
  const owned = new Map<string, string>()
  for (const project of projects) {
    if (path.resolve(project.rootDir) === resolvedWorkspaceDir) continue
    const ownedDir = resolveProjectOwnedModulesDir(project, opts)
    if (ownedDir != null) {
      owned.set(project.rootDir, ownedDir)
    }
  }
  return owned
}

function resolveProjectOwnedModulesDir (
  project: Project,
  opts: CleanWorkspaceDiscoveryOptions
): string | undefined {
  const rawLeaf = opts.hasPerProjectModulesDirs && project.manifest.name != null
    ? (opts.modulesDirsByProjectName?.[project.manifest.name] ?? opts.modulesDir)
    : opts.modulesDir
  if (rawLeaf == null || path.isAbsolute(rawLeaf)) return undefined
  const rel = path.relative('.', rawLeaf)
  const isBelow = rel !== '' && rel !== '..' && !rel.startsWith(`..${path.sep}`) && !path.isAbsolute(rel)
  return isBelow ? path.join(project.rootDir, rel) : undefined
}

function isInsideOwnedModulesDir (dir: string, ownedModulesDirs: Map<string, string>): boolean {
  for (let ancestor = path.dirname(dir); ; ancestor = path.dirname(ancestor)) {
    const modulesDir = ownedModulesDirs.get(ancestor)
    if (modulesDir != null && dir.startsWith(`${modulesDir}${path.sep}`)) return true
    if (path.dirname(ancestor) === ancestor) return false
  }
}

async function hasSymlinkComponentUnder (resolvedWorkspaceDir: string, targetDir: string): Promise<boolean> {
  const rel = path.relative(resolvedWorkspaceDir, path.resolve(targetDir))
  if (rel === '' || rel === '..' || rel.startsWith(`..${path.sep}`) || path.isAbsolute(rel)) {
    return false
  }
  const segments = rel.split(path.sep).filter((segment) => segment !== '' && segment !== '.')
  const stats = await Promise.all(segments.map(async (_, idx) => {
    const partial = path.join(resolvedWorkspaceDir, ...segments.slice(0, idx + 1))
    try {
      return await fs.lstat(partial)
    } catch (err: unknown) {
      if (isError(err) && 'code' in err && (err.code === 'ENOENT' || err.code === 'ENOTDIR')) {
        return undefined
      }
      throw err
    }
  }))
  return stats.some((stat) => stat?.isSymbolicLink() === true)
}

export async function collectExplicitLiteralMemberDirs (
  workspaceDir: string,
  patterns: string[] | undefined
): Promise<string[]> {
  const subpaths = literalPatternSubpaths(patterns)
  if (subpaths.length === 0) return []
  const resolvedWorkspaceDir = path.resolve(workspaceDir)
  const checks = await Promise.all(subpaths.map(async (subpath) => {
    const candidate = path.join(resolvedWorkspaceDir, ...subpath.split('/'))
    const keep = !await hasSymlinkComponentUnder(resolvedWorkspaceDir, candidate) &&
      await hasRegularManifestFile(candidate)
    return { candidate, keep }
  }))
  return checks.filter(({ keep }) => keep).map(({ candidate }) => candidate)
}

async function hasRegularManifestFile (dir: string): Promise<boolean> {
  const stats = await Promise.all(['package.json', 'package.json5', 'package.yaml'].map(async (name) => {
    try {
      return await fs.lstat(path.join(dir, name))
    } catch (err: unknown) {
      if (isError(err) && 'code' in err && (err.code === 'ENOENT' || err.code === 'ENOTDIR')) {
        return undefined
      }
      throw err
    }
  }))
  return stats.some((stat) => stat != null && !stat.isSymbolicLink() && stat.isFile())
}
