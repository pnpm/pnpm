import assert from 'node:assert'
import fs from 'node:fs/promises'
import path from 'node:path'
import util from 'node:util'

import { parseCatalogProtocol } from '@pnpm/catalogs.protocol-parser'
import { PnpmError } from '@pnpm/error'
import type { BaseManifest, ProjectRootDir } from '@pnpm/types'
import * as find from 'empathic/find'
import { safeExeca as execa } from 'execa'
import * as micromatch from 'micromatch'
import * as yaml from 'yaml'

type ChangeType = 'source' | 'test'

interface ChangedDir {
  dir: string, changeType: ChangeType
}

export async function getChangedProjects (
  projectDirs: ProjectRootDir[],
  commit: string,
  opts: {
    workspaceDir: string
    workspaceRoot?: string
    projects?: Array<{ rootDir: ProjectRootDir, manifest: BaseManifest }>
    testPattern?: string[]
    changedFilesIgnorePattern?: string[]
  }
): Promise<[ProjectRootDir[], ProjectRootDir[]]> {

  // .git is a directory in regular repos, but a file in worktrees. The
  // nearest entry of either kind wins, so a worktree checked out inside
  // another repository's tree resolves to the worktree root, matching
  // where git anchors its diff paths.
  const gitPath = find.up('.git', { cwd: opts.workspaceDir })

  const repoRoot = path.resolve(gitPath ?? opts.workspaceDir, '..')

  const changedDirs = (await getChangedDirsSinceCommit(commit, opts.workspaceDir, opts.testPattern ?? [], opts.changedFilesIgnorePattern ?? []))
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
  if (opts.projects != null) {
    const catalogChangedProjects = await getProjectsUsingChangedCatalogEntries(
      commit,
      repoRoot,
      opts.workspaceRoot ?? opts.workspaceDir,
      opts.projects
    )
    for (const projectDir of catalogChangedProjects) {
      projectChangeTypes.set(projectDir, 'source')
    }
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

interface WorkspaceCatalogsManifest {
  catalog?: Record<string, string>
  catalogs?: Record<string, Record<string, string>>
}

async function getProjectsUsingChangedCatalogEntries (
  commit: string,
  repoRoot: string,
  workspaceRoot: string,
  projects: Array<{ rootDir: ProjectRootDir, manifest: BaseManifest }>
): Promise<ProjectRootDir[]> {
  const manifestPath = path.join(workspaceRoot, 'pnpm-workspace.yaml')
  const relativeManifestPath = path.relative(repoRoot, manifestPath).split(path.sep).join('/')
  const [before, after] = await Promise.all([
    readWorkspaceCatalogsAtCommit(repoRoot, commit, relativeManifestPath),
    readWorkspaceCatalogs(manifestPath),
  ])
  const changedEntries = changedCatalogEntries(before, after)
  if (changedEntries.size === 0) return []

  const changedProjects = new Set<ProjectRootDir>()
  for (const project of projects) {
    for (const dependencyField of ['dependencies', 'devDependencies', 'optionalDependencies', 'peerDependencies'] as const) {
      for (const [dependencyName, specifier] of Object.entries(project.manifest[dependencyField] ?? {})) {
        const catalogName = parseCatalogProtocol(specifier)
        if (catalogName != null && changedEntries.has(`${catalogName}\0${dependencyName}`)) {
          changedProjects.add(project.rootDir)
        }
      }
    }
  }
  return [...changedProjects]
}

async function readWorkspaceCatalogsAtCommit (
  repoRoot: string,
  commit: string,
  relativeManifestPath: string
): Promise<Record<string, Record<string, string>>> {
  try {
    const { stdout } = await execa('git', ['show', `${commit}:${relativeManifestPath}`], { cwd: repoRoot })
    return parseWorkspaceCatalogs(stdout as string)
  } catch {
    return {}
  }
}

async function readWorkspaceCatalogs (manifestPath: string): Promise<Record<string, Record<string, string>>> {
  try {
    return parseWorkspaceCatalogs(await fs.readFile(manifestPath, 'utf8'))
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') return {}
    throw err
  }
}

function parseWorkspaceCatalogs (source: string): Record<string, Record<string, string>> {
  const manifest = (yaml.parse(source) ?? {}) as WorkspaceCatalogsManifest
  return {
    ...(manifest.catalog != null ? { default: manifest.catalog } : {}),
    ...(manifest.catalogs ?? {}),
  }
}

function changedCatalogEntries (
  before: Record<string, Record<string, string>>,
  after: Record<string, Record<string, string>>
): Set<string> {
  const changed = new Set<string>()
  for (const catalogName of new Set([...Object.keys(before), ...Object.keys(after)])) {
    const beforeCatalog = before[catalogName] ?? {}
    const afterCatalog = after[catalogName] ?? {}
    for (const dependencyName of new Set([...Object.keys(beforeCatalog), ...Object.keys(afterCatalog)])) {
      if (beforeCatalog[dependencyName] !== afterCatalog[dependencyName]) {
        changed.add(`${catalogName}\0${dependencyName}`)
      }
    }
  }
  return changed
}

async function getChangedDirsSinceCommit (commit: string, workingDir: string, testPattern: string[], changedFilesIgnorePattern: string[]): Promise<ChangedDir[]> {
  let diff!: string
  try {
    diff = (
      await execa('git', [
        'diff',
        '--name-only',
        // Keeps an option-like `<since>` (`--output=...`) from being
        // parsed as a git option — git rejects it as a bad revision.
        '--end-of-options',
        commit,
        '--',
        workingDir,
      ], { cwd: workingDir })
    ).stdout as string
  } catch (err: unknown) {
    assert(util.types.isNativeError(err))
    throw new PnpmError('FILTER_CHANGED', `Filtering by changed packages failed. ${'stderr' in err ? err.stderr as string : ''}`)
  }
  const changedDirs = new Map<string, ChangeType>()

  if (!diff) {
    return []
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

  for (const changedFile of changedFiles) {
    const dir = path.dirname(changedFile)

    if (changedDirs.get(dir) === 'source') continue

    const changeType: ChangeType = testPattern.some(pattern => micromatch.default.isMatch(changedFile, pattern))
      ? 'test'
      : 'source'
    changedDirs.set(dir, changeType)
  }

  return Array.from(changedDirs.entries()).map(([dir, changeType]) => ({ dir, changeType }))
}
