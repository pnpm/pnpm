import PnpmError from '@pnpm/error'
import minimatch from 'minimatch'
import path = require('path')
import execa = require('execa')
import findUp = require('find-up')
import isSubdir = require('is-subdir')

export default async function changedSince (packageDirs: string[], commit: string, opts: { workspaceDir: string, filterPattern?: string }): Promise<[string[], string[]]> {
  const repoRoot = path.resolve(await findUp('.git', { cwd: opts.workspaceDir, type: 'directory' }) ?? opts.workspaceDir, '..')
  const [dirsWithChanges, dirsMatchingFilter] = await getChangedDirsSinceCommit(commit, opts.workspaceDir, opts.filterPattern)
  let changedDirs = Array.from(dirsWithChanges).map(changedDir => path.join(repoRoot, changedDir))
  const changedPkgs: string[] = []
  for (const packageDir of packageDirs.sort((pkgDir1, pkgDir2) => pkgDir2.length - pkgDir1.length)) {
    if (
      changedDirs.some(changedDir => isSubdir(packageDir, changedDir))
    ) {
      changedDirs = changedDirs.filter((changedDir) => !isSubdir(packageDir, changedDir))
      changedPkgs.push(packageDir)
    }
  }

  const ignoreDependentForPkgs = Array.from(dirsMatchingFilter)
    .map(dir => path.join(repoRoot, dir))
    .map(dir => changedPkgs.find(pkg => dir.startsWith(pkg)))
    .filter(dir => !!dir)

  return [changedPkgs, ignoreDependentForPkgs as string[]]
}

async function getChangedDirsSinceCommit (commit: string, workingDir: string, filterPattern?: string) {
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
  } catch (err) {
    throw new PnpmError('FILTER_CHANGED', `Filtering by changed packages failed. ${err.stderr as string}`)
  }
  const changedDirs = new Set<string>()
  const dirsMatchingFilter = new Set<string>()

  if (!diff) {
    return [changedDirs, dirsMatchingFilter]
  }
  const changedFiles = diff.split('\n')

  for (const changedFile of changedFiles) {
    const dirName  = path.dirname(changedFile)

    changedDirs.add(dirName)

    if (filterPattern && minimatch(changedFile, filterPattern)) {
      dirsMatchingFilter.add(dirName)
    }
  }

  return [changedDirs, dirsMatchingFilter]
}
