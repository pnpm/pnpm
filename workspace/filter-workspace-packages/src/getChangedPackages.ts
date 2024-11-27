import assert from 'assert'
import path from 'path'
import util from 'util'
import { PnpmError } from '@pnpm/error'
import * as micromatch from 'micromatch'
import execa from 'execa'
import findUp from 'find-up'
import { type ProjectRootDir } from '@pnpm/types'

type ChangeType = 'source' | 'test'

interface ChangedDir { dir: string, changeType: ChangeType }

export async function getChangedPackages (
  packageDirs: ProjectRootDir[],
  commit: string,
  opts: { workspaceDir: string, testPattern?: string[], changedFilesIgnorePattern?: string[] }
): Promise<[ProjectRootDir[], ProjectRootDir[]]> {
  const repoRoot = path.resolve(await findUp('.git', { cwd: opts.workspaceDir, type: 'directory' }) ?? opts.workspaceDir, '..')
  const changedDirs = (await getChangedDirsSinceCommit(commit, opts.workspaceDir, opts.testPattern ?? [], opts.changedFilesIgnorePattern ?? []))
    .map(changedDir => ({ ...changedDir, dir: path.join(repoRoot, changedDir.dir) }))
  const pkgChangeTypes = new Map<ProjectRootDir, ChangeType | undefined>()
  for (const pkgDir of packageDirs) {
    pkgChangeTypes.set(pkgDir, undefined)
  }
  for (const changedDir of changedDirs) {
    let currentDir = changedDir.dir
    while (!pkgChangeTypes.has(currentDir as ProjectRootDir)) {
      const nextDir = path.dirname(currentDir)
      if (nextDir === currentDir) break
      currentDir = nextDir
    }
    if (pkgChangeTypes.get(currentDir as ProjectRootDir) === 'source') continue
    pkgChangeTypes.set(currentDir as ProjectRootDir, changedDir.changeType)
  }

  const changedPkgs = [] as ProjectRootDir[]
  const ignoreDependentForPkgs = [] as ProjectRootDir[]
  for (const [changedDir, changeType] of pkgChangeTypes.entries()) {
    switch (changeType) {
    case 'source':
      changedPkgs.push(changedDir)
      break
    case 'test':
      ignoreDependentForPkgs.push(changedDir)
      break
    }
  }
  return [changedPkgs, ignoreDependentForPkgs]
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
    ).stdout
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
    ? micromatch.not(allChangedFiles, patterns, {
      dot: true,
    })
    : allChangedFiles

  for (const changedFile of changedFiles) {
    const dir = path.dirname(changedFile)

    if (changedDirs.get(dir) === 'source') continue

    const changeType: ChangeType = testPattern.some(pattern => micromatch.isMatch(changedFile, pattern))
      ? 'test'
      : 'source'
    changedDirs.set(dir, changeType)
  }

  return Array.from(changedDirs.entries()).map(([dir, changeType]) => ({ dir, changeType }))
}
