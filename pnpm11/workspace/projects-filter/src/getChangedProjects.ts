import assert from 'node:assert'
import fs from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import { getCatalogsFromWorkspaceManifest } from '@pnpm/catalogs.config'
import { parseCatalogProtocol } from '@pnpm/catalogs.protocol-parser'
import { PnpmError } from '@pnpm/error'
import type { BaseManifest, ProjectRootDir } from '@pnpm/types'
import * as find from 'empathic/find'
import { safeExeca as execa } from 'execa'
import * as micromatch from 'micromatch'
import * as yaml from 'yaml'

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

  // .git is a directory in regular repos, but a file in worktrees. The
  // nearest entry of either kind wins, so a worktree checked out inside
  // another repository's tree resolves to the worktree root, matching
  // where git anchors its diff paths.
  const gitPath = find.up('.git', { cwd: opts.workspaceDir })

  const repoRoot = path.resolve(gitPath ?? opts.workspaceDir, '..')

  const base = await getMergeBase(commit, opts.workspaceDir)
  const { changedDirs: rawChangedDirs, workspaceManifestChanged } = await getChangedDirsSinceCommit(
    base,
    workingDir,
    repoRoot,
    opts.testPattern ?? [],
    opts.changedFilesIgnorePattern ?? [],
    opts.workspaceDir
  )

  const changedDirs = rawChangedDirs
    .map(changedDir => ({ ...changedDir, dir: path.join(repoRoot, changedDir.dir) }))
  const projectChangeTypes = new Map<ProjectRootDir, ChangeType | undefined>()
  for (const projectDir of projectDirs) {
    projectChangeTypes.set(projectDir, undefined)
  }
  for (const changedDir of changedDirs) {
    let currentDir = changedDir.dir
    while (!projectChangeTypes.has(currentDir as ProjectRootDir)) {
      const nextDir = path.dirname(currentDir)
      if (nextDir === currentDir) break
      currentDir = nextDir
    }
    if (projectChangeTypes.get(currentDir as ProjectRootDir) === 'source') continue
    projectChangeTypes.set(currentDir as ProjectRootDir, changedDir.changeType)
  }

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
    }
  }
  return [changedProjects, ignoreDependentForPkgs]
}

function isSubdir (parent: string, child: string): boolean {
  const rel = path.relative(parent, child)
  return !rel.startsWith('..') && !path.isAbsolute(rel)
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
  const formattedFilter = workingDir.replace(/\\/g, '/').replace(/\/$/, '')
  if (micromatch.default.isMatch(projectDir, formattedFilter, { format })) {
    return true
  }
  if (formattedFilter.endsWith('/*')) {
    const recursivePattern = `${formattedFilter}*`
    return micromatch.default.isMatch(projectDir, recursivePattern, { format })
  }
  return false
}

async function applyCatalogChangesToProjects (params: {
  allProjects?: Array<{ rootDir: ProjectRootDir, manifest: BaseManifest }>
  commit: string
  projectChangeTypes: Map<ProjectRootDir, ChangeType | undefined>
  projectDirs: ProjectRootDir[]
  repoRoot: string
  useGlobDirFiltering?: boolean
  workingDir: string
  workspaceDir: string
}): Promise<void> {
  const relManifestPath = path.relative(params.repoRoot, path.join(params.workspaceDir, 'pnpm-workspace.yaml')).replaceAll('\\', '/')

  let prevManifestContent = ''
  try {
    const result = await execa('git', [
      'show',
      '--end-of-options',
      `${params.commit}:${relManifestPath}`,
    ], { cwd: params.workspaceDir, env: { ...process.env, LC_ALL: 'C' } })
    prevManifestContent = result.stdout as string
  } catch (err: unknown) {
    const stderr = (err as { stderr?: string }).stderr ?? ''
    if (stderr.includes('does not exist in') || stderr.includes('exists on disk, but not in')) {
      prevManifestContent = ''
    } else {
      throw err
    }
  }

  let currManifestContent = ''
  try {
    currManifestContent = await fs.promises.readFile(path.join(params.workspaceDir, 'pnpm-workspace.yaml'), 'utf8')
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && (err as NodeJS.ErrnoException).code === 'ENOENT') {
      currManifestContent = ''
    } else {
      throw err
    }
  }

  type Catalogs = ReturnType<typeof getCatalogsFromWorkspaceManifest>
  let prevCatalogs: Catalogs = {}
  let currCatalogs: Catalogs = {}
  if (prevManifestContent) {
    prevCatalogs = getCatalogsFromWorkspaceManifest(yaml.parse(prevManifestContent))
  }
  if (currManifestContent) {
    currCatalogs = getCatalogsFromWorkspaceManifest(yaml.parse(currManifestContent))
  }

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

async function loadProjects (
  projectDirs: ProjectRootDir[],
  allProjects?: Array<{ rootDir: ProjectRootDir, manifest: BaseManifest }>
): Promise<Array<{ rootDir: ProjectRootDir, manifest: BaseManifest }>> {
  if (allProjects != null && allProjects.length > 0) {
    const projectDirSet = new Set(projectDirs)
    return allProjects.filter(p => projectDirSet.has(p.rootDir))
  }
  return Promise.all(
    projectDirs.map(async (rootDir) => {
      let manifestContent = ''
      try {
        manifestContent = await fs.promises.readFile(path.join(rootDir, 'package.json'), 'utf8')
      } catch (err: unknown) {
        if (util.types.isNativeError(err) && (err as NodeJS.ErrnoException).code === 'ENOENT') {
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
  prevCatalogs: ReturnType<typeof getCatalogsFromWorkspaceManifest>,
  currCatalogs: ReturnType<typeof getCatalogsFromWorkspaceManifest>
): Map<string, Set<string>> {
  const changed = new Map<string, Set<string>>()
  const allCatalogNames = new Set([
    ...Object.keys(prevCatalogs),
    ...Object.keys(currCatalogs),
  ])

  for (const catalogName of allCatalogNames) {
    const prevCatalog = prevCatalogs[catalogName] ?? {}
    const currCatalog = currCatalogs[catalogName] ?? {}
    const allDepNames = new Set([
      ...Object.keys(prevCatalog),
      ...Object.keys(currCatalog),
    ])

    for (const depName of allDepNames) {
      if (prevCatalog[depName] !== currCatalog[depName]) {
        let deps = changed.get(catalogName)
        if (!deps) {
          deps = new Set()
          changed.set(catalogName, deps)
        }
        deps.add(depName)
      }
    }
  }

  return changed
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

  for (const deps of depFields) {
    if (!deps) continue
    for (const [depName, specifier] of Object.entries(deps)) {
      if (typeof specifier !== 'string') continue
      const { catalogName, lookupName } = parseCatalogDep(depName, specifier)
      if (catalogName != null) {
        if (changedCatalogs.get(catalogName)?.has(lookupName)) {
          return true
        }
      }
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
async function getMergeBase (commit: string, workingDir: string): Promise<string> {
  try {
    const { stdout } = await execa('git', ['merge-base', '--end-of-options', commit, 'HEAD'], { cwd: workingDir })
    return (stdout as string).trim() || commit
  } catch (err: unknown) {
    assert(util.types.isNativeError(err))
    const exitCode = 'exitCode' in err ? err.exitCode : undefined
    if (exitCode === 1 || exitCode === 128) return commit
    throw new PnpmError('FILTER_CHANGED', `Filtering by changed packages failed. ${'stderr' in err && err.stderr ? err.stderr as string : err.message}`, { cause: err })
  }
}

async function getChangedDirsSinceCommit (
  commit: string,
  workingDir: string,
  repoRoot: string,
  testPattern: string[],
  changedFilesIgnorePattern: string[],
  workspaceDir: string
): Promise<{ changedDirs: ChangedDir[], workspaceManifestChanged: boolean }> {
  const workspaceManifestPath = path.resolve(workspaceDir, 'pnpm-workspace.yaml')
  const diffPaths = workingDir === workspaceDir
    ? [workingDir]
    : [workingDir, workspaceManifestPath]

  let diff!: string
  try {
    diff = (
      await execa('git', [
        'diff',
        '--name-only',
        '--no-relative',
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
    assert(util.types.isNativeError(err))
    throw new PnpmError('FILTER_CHANGED', `Filtering by changed packages failed. ${'stderr' in err ? err.stderr as string : ''}`)
  }
  const changedDirs = new Map<string, ChangeType>()

  if (!diff) {
    return { changedDirs: [], workspaceManifestChanged: false }
  }

  const allChangedFiles = diff.split('\n')
    // The prefix and suffix '"' are appended to the Korean path
    .map(line => line.replace(/^"/, '').replace(/"$/, ''))
  const patterns = changedFilesIgnorePattern.filter(
    (pattern) => pattern.length
  )
  const changedFiles = (patterns.length > 0)
    ? micromatch.default.not(allChangedFiles, patterns, {
      dot: true,
    })
    : allChangedFiles

  let workspaceManifestChanged = false

  for (const changedFile of changedFiles) {
    if (!changedFile) continue
    if (path.resolve(repoRoot, changedFile) === workspaceManifestPath) {
      workspaceManifestChanged = true
      if (workingDir !== workspaceDir) continue
    }
    const dir = path.dirname(changedFile)

    if (changedDirs.get(dir) === 'source') continue

    const changeType: ChangeType = testPattern.some(pattern => micromatch.default.isMatch(changedFile, pattern))
      ? 'test'
      : 'source'
    changedDirs.set(dir, changeType)
  }

  return {
    changedDirs: Array.from(changedDirs.entries()).map(([dir, changeType]) => ({ dir, changeType })),
    workspaceManifestChanged,
  }
}
