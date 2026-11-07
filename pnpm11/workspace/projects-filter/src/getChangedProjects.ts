import assert from 'node:assert'
import fs from 'node:fs'
import path from 'node:path'

import { getCatalogsFromWorkspaceManifest } from '@pnpm/catalogs.config'
import { parseCatalogProtocol } from '@pnpm/catalogs.protocol-parser'
import { isError, PnpmError } from '@pnpm/error'
import type { BaseManifest, ProjectRootDir } from '@pnpm/types'
import * as find from 'empathic/find'
import { safeExeca as execa } from 'execa'
import * as micromatch from 'micromatch'
import * as yaml from 'yaml'

import { formatDirGlob, formatDirGlobCandidate } from './dirGlob.js'
import { checkGitVersion, getGitVersion, gitSupportsNoRelative } from './gitVersion.js'

type ChangeType = 'source' | 'test'

interface ChangedDir {
  dir: string
  changeType: ChangeType
}

export interface GetChangedProjectsOptions {
  workspaceDir: string
  workingDir?: string
  testPattern?: string[]
  changedFilesIgnorePattern?: string[]
  allProjects?: Array<{ rootDir: ProjectRootDir, manifest: BaseManifest }>
  useGlobDirFiltering?: boolean
}

export async function getChangedProjects (
  projectDirs: ProjectRootDir[],
  commit: string,
  opts: GetChangedProjectsOptions
): Promise<[ProjectRootDir[], ProjectRootDir[]]> {
  const workingDir = opts.workingDir ?? opts.workspaceDir
  const repoRoot = findRepoRoot(opts.workspaceDir)

  checkGitVersion(await getGitVersion())
  const base = await getMergeBase(commit, opts.workspaceDir)
  const { changedDirs, workspaceManifestChanged } = await getChangedDirsSinceCommit({
    commit: base,
    workingDir,
    repoRoot,
    testPattern: opts.testPattern ?? [],
    changedFilesIgnorePattern: opts.changedFilesIgnorePattern ?? [],
    workspaceDir: opts.workspaceDir,
  })

  const projectChangeTypes = assignChangedDirsToProjects(projectDirs, changedDirs, repoRoot)

  if (workspaceManifestChanged) {
    await applyCatalogChangesToProjects({
      allProjects: opts.allProjects,
      commit: base,
      projectChangeTypes,
      projectDirs,
      repoRoot,
      useGlobDirFiltering: opts.useGlobDirFiltering,
      workingDir,
      workspaceDir: opts.workspaceDir,
    })
  }

  return partitionProjectsByChangeType(projectChangeTypes)
}

function findRepoRoot (workspaceDir: string): string {
  // .git is a directory in regular repos, but a file in worktrees. The
  // nearest entry of either kind wins, so a worktree checked out inside
  // another repository's tree resolves to the worktree root, matching
  // where git anchors its diff paths.
  const gitPath = find.up('.git', { cwd: workspaceDir })

  return path.resolve(gitPath ?? workspaceDir, '..')
}

type ProjectChangeTypes = Map<ProjectRootDir, ChangeType | undefined>

function assignChangedDirsToProjects (
  projectDirs: ProjectRootDir[],
  rawChangedDirs: ChangedDir[],
  repoRoot: string
): ProjectChangeTypes {
  const projectChangeTypes: ProjectChangeTypes = new Map()
  for (const projectDir of projectDirs) {
    projectChangeTypes.set(projectDir, undefined)
  }
  for (const changedDir of rawChangedDirs) {
    const projectDir = findOwningProjectDir(path.join(repoRoot, changedDir.dir), projectChangeTypes)
    if (projectChangeTypes.get(projectDir) === 'source') continue
    projectChangeTypes.set(projectDir, changedDir.changeType)
  }
  return projectChangeTypes
}

function findOwningProjectDir (dir: string, projectChangeTypes: ProjectChangeTypes): ProjectRootDir {
  let currentDir = dir
  while (!projectChangeTypes.has(currentDir as ProjectRootDir)) {
    const nextDir = path.dirname(currentDir)
    if (nextDir === currentDir) break
    currentDir = nextDir
  }
  return currentDir as ProjectRootDir
}

function partitionProjectsByChangeType (projectChangeTypes: ProjectChangeTypes): [ProjectRootDir[], ProjectRootDir[]] {
  const changedProjects = [] as ProjectRootDir[]
  const ignoreDependentForPkgs = [] as ProjectRootDir[]
  for (const [changedDir, changeType] of projectChangeTypes.entries()) {
    switch (changeType) {
      case 'source':
        changedProjects.push(changedDir)
        break
      case 'test':
        ignoreDependentForPkgs.push(changedDir)
        break
      case undefined:
        break
    }
  }
  return [changedProjects, ignoreDependentForPkgs]
}

function isSubdir (parent: string, child: string): boolean {
  const rel = path.relative(parent, child)
  return rel !== '..' && !rel.startsWith(`..${path.sep}`) && !path.isAbsolute(rel)
}

function projectMatchesWorkingDir (
  workingDir: string,
  projectDir: string,
  useGlobDirFiltering?: boolean
): boolean {
  if (isSubdir(workingDir, projectDir) || projectDir === workingDir) {
    return true
  }
  if (!useGlobDirFiltering) {
    return false
  }
  const format = (str: string) => str.replace(/\/$/, '')
  const formattedFilter = formatDirGlob(workingDir)
  const candidate = formatDirGlobCandidate(projectDir)
  if (micromatch.default.isMatch(candidate, formattedFilter, { format })) {
    return true
  }
  if (formattedFilter.endsWith('/*')) {
    const recursivePattern = `${formattedFilter}*`
    return micromatch.default.isMatch(candidate, recursivePattern, { format })
  }
  return false
}

interface ApplyCatalogChangesParams {
  allProjects?: Array<{ rootDir: ProjectRootDir, manifest: BaseManifest }>
  commit: string
  projectChangeTypes: ProjectChangeTypes
  projectDirs: ProjectRootDir[]
  repoRoot: string
  useGlobDirFiltering?: boolean
  workingDir: string
  workspaceDir: string
}

type Catalogs = ReturnType<typeof getCatalogsFromWorkspaceManifest>

async function applyCatalogChangesToProjects (params: ApplyCatalogChangesParams): Promise<void> {
  const prevManifestContent = await readWorkspaceManifestAtCommit(params)
  const currManifestContent = await readCurrentWorkspaceManifest(params.workspaceDir)

  const prevCatalogs = parseCatalogs(prevManifestContent)
  const currCatalogs = parseCatalogs(currManifestContent)

  const changedCatalogs = getChangedCatalogEntries(prevCatalogs, currCatalogs)
  if (changedCatalogs.size === 0) return

  const projects = await loadProjects(params.projectDirs, params.allProjects)
  for (const project of projects) {
    if (params.workingDir !== params.workspaceDir && !projectMatchesWorkingDir(params.workingDir, project.rootDir, params.useGlobDirFiltering)) {
      continue
    }
    if (params.projectChangeTypes.get(project.rootDir) === 'source') continue
    if (projectUsesChangedCatalogs(project.manifest, changedCatalogs)) {
      params.projectChangeTypes.set(project.rootDir, 'source')
    }
  }
}

async function readWorkspaceManifestAtCommit (
  params: Pick<ApplyCatalogChangesParams, 'commit' | 'repoRoot' | 'workspaceDir'>
): Promise<string> {
  const relManifestPath = path.relative(params.repoRoot, path.join(params.workspaceDir, 'pnpm-workspace.yaml')).replaceAll('\\', '/')

  try {
    const result = await execa('git', [
      'show',
      '--end-of-options',
      `${params.commit}:${relManifestPath}`,
    ], { cwd: params.workspaceDir, env: { ...process.env, LC_ALL: 'C' } })
    return result.stdout as string
  } catch (err: unknown) {
    const stderr = (err as { stderr?: string }).stderr ?? ''
    if (stderr.includes('does not exist in') || stderr.includes('exists on disk, but not in')) {
      return ''
    }
    throw err
  }
}

async function readCurrentWorkspaceManifest (workspaceDir: string): Promise<string> {
  try {
    return await fs.promises.readFile(path.join(workspaceDir, 'pnpm-workspace.yaml'), 'utf8')
  } catch (err: unknown) {
    if (isError(err) && (err as NodeJS.ErrnoException).code === 'ENOENT') {
      return ''
    }
    throw err
  }
}

function parseCatalogs (manifestContent: string): Catalogs {
  if (!manifestContent) return {}
  return getCatalogsFromWorkspaceManifest(yaml.parse(manifestContent))
}

async function loadProjects (
  projectDirs: ProjectRootDir[],
  allProjects?: Array<{ rootDir: ProjectRootDir, manifest: BaseManifest }>
): Promise<Array<{ rootDir: ProjectRootDir, manifest: BaseManifest }>> {
  if (allProjects != null && allProjects.length > 0) {
    const projectDirSet = new Set(projectDirs)
    return allProjects.filter(project => projectDirSet.has(project.rootDir))
  }
  return Promise.all(
    projectDirs.map(async (rootDir) => {
      let manifestContent = ''
      try {
        manifestContent = await fs.promises.readFile(path.join(rootDir, 'package.json'), 'utf8')
      } catch (err: unknown) {
        if (isError(err) && (err as NodeJS.ErrnoException).code === 'ENOENT') {
          return {
            rootDir,
            manifest: {} as BaseManifest,
          }
        }
        throw err
      }
      return {
        rootDir,
        manifest: JSON.parse(manifestContent) as BaseManifest,
      }
    })
  )
}

function getChangedCatalogEntries (
  prevCatalogs: Catalogs,
  currCatalogs: Catalogs
): Map<string, Set<string>> {
  const changed = new Map<string, Set<string>>()
  const allCatalogNames = new Set([
    ...Object.keys(prevCatalogs),
    ...Object.keys(currCatalogs),
  ])

  for (const catalogName of allCatalogNames) {
    const changedDepNames = getChangedDepNames(prevCatalogs[catalogName] ?? {}, currCatalogs[catalogName] ?? {})
    if (changedDepNames.size > 0) {
      changed.set(catalogName, changedDepNames)
    }
  }

  return changed
}

function getChangedDepNames (
  prevCatalog: Record<string, string | undefined>,
  currCatalog: Record<string, string | undefined>
): Set<string> {
  const allDepNames = new Set([
    ...Object.keys(prevCatalog),
    ...Object.keys(currCatalog),
  ])
  const changedDepNames = new Set<string>()
  for (const depName of allDepNames) {
    if (prevCatalog[depName] !== currCatalog[depName]) {
      changedDepNames.add(depName)
    }
  }
  return changedDepNames
}

function projectUsesChangedCatalogs (
  manifest: BaseManifest,
  changedCatalogs: Map<string, Set<string>>
): boolean {
  const depFields = [
    manifest.dependencies,
    manifest.devDependencies,
    manifest.optionalDependencies,
    manifest.peerDependencies,
  ]

  return depFields.some((deps) => deps != null && depsUseChangedCatalogs(deps, changedCatalogs))
}

function depsUseChangedCatalogs (
  deps: Record<string, string>,
  changedCatalogs: Map<string, Set<string>>
): boolean {
  for (const [depName, specifier] of Object.entries(deps)) {
    if (typeof specifier !== 'string') continue
    const { catalogName, lookupName } = parseCatalogDep(depName, specifier)
    if (catalogName != null && changedCatalogs.get(catalogName)?.has(lookupName)) {
      return true
    }
  }
  return false
}

function parseCatalogDep (depName: string, specifier: string): { catalogName: string | null, lookupName: string } {
  let catalogName = parseCatalogProtocol(specifier)
  let lookupName = depName
  if (catalogName == null && specifier.startsWith('npm:')) {
    const lastAtIndex = specifier.lastIndexOf('@')
    if (lastAtIndex > 4) {
      catalogName = parseCatalogProtocol(specifier.slice(lastAtIndex + 1))
      if (catalogName != null) {
        lookupName = specifier.slice(4, lastAtIndex)
      }
    }
  }
  return { catalogName, lookupName }
}

// Diffing against the merge base keeps commits made only on the `<since>`
// side out of the result. git exits with 1 when there is no merge base (a
// shallow clone or unrelated histories) and with 128 for an invalid
// `<since>`. Both fall back to diffing `<since>` itself, which reports the
// bad revision.
async function getMergeBase (commit: string, workspaceDir: string): Promise<string> {
  try {
    const { stdout } = await execa('git', ['merge-base', '--end-of-options', commit, 'HEAD'], { cwd: workspaceDir })
    return (stdout as string).trim() || commit
  } catch (err: unknown) {
    assert(isError(err))
    const exitCode = 'exitCode' in err ? err.exitCode : undefined
    if (exitCode === 1 || exitCode === 128) return commit
    throw new PnpmError('FILTER_CHANGED', `Filtering by changed packages failed. ${'stderr' in err && err.stderr ? err.stderr as string : err.message}`, { cause: err })
  }
}

interface GetChangedDirsSinceCommitOptions {
  commit: string
  workingDir: string
  repoRoot: string
  testPattern: string[]
  changedFilesIgnorePattern: string[]
  workspaceDir: string
}

async function getChangedDirsSinceCommit (
  opts: GetChangedDirsSinceCommitOptions
): Promise<{ changedDirs: ChangedDir[], workspaceManifestChanged: boolean }> {
  const workspaceManifestPath = path.resolve(opts.workspaceDir, 'pnpm-workspace.yaml')
  const diffPaths = opts.workingDir === opts.workspaceDir
    ? [opts.workingDir]
    : [opts.workingDir, workspaceManifestPath]

  const diff = await diffFileNames(opts.commit, diffPaths, opts.workspaceDir)

  if (!diff) {
    return { changedDirs: [], workspaceManifestChanged: false }
  }

  const changedFiles = filterOutIgnoredFiles(parseDiffFileNames(diff), opts.changedFilesIgnorePattern)
  return classifyChangedFiles(changedFiles, { ...opts, workspaceManifestPath })
}

function classifyChangedFiles (
  changedFiles: string[],
  opts: GetChangedDirsSinceCommitOptions & { workspaceManifestPath: string }
): { changedDirs: ChangedDir[], workspaceManifestChanged: boolean } {
  const changedDirs = new Map<string, ChangeType>()
  let workspaceManifestChanged = false

  for (const changedFile of changedFiles) {
    if (!changedFile) continue
    if (path.resolve(opts.repoRoot, changedFile) === opts.workspaceManifestPath) {
      workspaceManifestChanged = true
      if (opts.workingDir !== opts.workspaceDir) continue
    }
    const dir = path.dirname(changedFile)

    if (changedDirs.get(dir) === 'source') continue

    changedDirs.set(dir, getChangeType(changedFile, opts.testPattern))
  }

  return {
    changedDirs: Array.from(changedDirs.entries()).map(([dir, changeType]) => ({ dir, changeType })),
    workspaceManifestChanged,
  }
}

async function diffFileNames (commit: string, diffPaths: string[], workspaceDir: string): Promise<string> {
  try {
    return (
      await execa('git', [
        'diff',
        '--name-only',
        // NUL-terminated names are printed verbatim. Newline-terminated
        // ones are C-quoted with octal escapes when they contain non-ASCII
        // characters.
        '-z',
        ...(gitSupportsNoRelative(await getGitVersion()) ? ['--no-relative'] : []),
        '--no-renames',
        // Keeps an option-like `<since>` (`--output=...`) from being
        // parsed as a git option — git rejects it as a bad revision.
        '--end-of-options',
        commit,
        '--',
        ...diffPaths,
      ], { cwd: workspaceDir })
    ).stdout as string
  } catch (err: unknown) {
    assert(isError(err))
    throw new PnpmError('FILTER_CHANGED', `Filtering by changed packages failed. ${'stderr' in err ? err.stderr as string : ''}`)
  }
}

function parseDiffFileNames (diff: string): string[] {
  return diff.split('\0')
}

function filterOutIgnoredFiles (changedFiles: string[], changedFilesIgnorePattern: string[]): string[] {
  const patterns = changedFilesIgnorePattern.filter(
    (pattern) => pattern.length
  )
  return (patterns.length > 0)
    ? micromatch.default.not(changedFiles, patterns, {
      dot: true,
    })
    : changedFiles
}

function getChangeType (changedFile: string, testPattern: string[]): ChangeType {
  return testPattern.some(pattern => micromatch.default.isMatch(changedFile, pattern))
    ? 'test'
    : 'source'
}
