import assert from 'node:assert'
import path from 'node:path'
import util from 'node:util'

import { PnpmError } from '@pnpm/error'
import type { ProjectRootDir } from '@pnpm/types'
import { safeExeca as execa } from 'execa'
import { findUp } from 'find-up'
import * as micromatch from 'micromatch'

type ChangeType = 'source' | 'test'

interface ChangedDir {
  dir: string, changeType: ChangeType
}

export async function getChangedProjects (
  projectDirs: ProjectRootDir[],
  commit: string,
  opts: { workspaceDir: string, testPattern?: string[], changedFilesIgnorePattern?: string[] }
): Promise<[ProjectRootDir[], ProjectRootDir[]]> {

  // .git is a directory in regular repos, but a file in worktrees
  const gitPath = await findUp('.git', { cwd: opts.workspaceDir, type: 'directory' }) ??
                  await findUp('.git', { cwd: opts.workspaceDir, type: 'file' })

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

async function getChangedDirsSinceCommit (commit: string, workingDir: string, testPattern: string[], changedFilesIgnorePattern: string[]): Promise<ChangedDir[]> {
  let diff!: string
  try {
    diff = (
      await execa('git', [
        'diff',
        '--name-only',
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
