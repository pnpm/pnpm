import PnpmError from '@pnpm/error'
import path = require('path')
import execa = require('execa')
import findUp = require('find-up')
import isSubdir = require('is-subdir')

export default async function changedSince (packageDirs: string[], commit: string, opts: { workspaceDir: string }): Promise<string[]> {
  const repoRoot = path.resolve(await findUp('.git', { cwd: opts.workspaceDir, type: 'directory' }) ?? opts.workspaceDir, '..')
  let changedDirs = Array.from(
    await getChangedDirsSinceCommit(commit, opts.workspaceDir)
  ).map(changedDir => path.join(repoRoot, changedDir))
  const changedPkgs = []
  for (const packageDir of packageDirs.sort((pkgDir1, pkgDir2) => pkgDir2.length - pkgDir1.length)) {
    if (
      changedDirs.some(changedDir => isSubdir(packageDir, changedDir))
    ) {
      changedDirs = changedDirs.filter((changedDir) => !isSubdir(packageDir, changedDir))
      changedPkgs.push(packageDir)
    }
  }
  return changedPkgs
}

async function getChangedDirsSinceCommit (commit: string, workingDir: string) {
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

  if (!diff) {
    return changedDirs
  }
  const changedFiles = diff.split('\n')

  for (const changedFile of changedFiles) {
    changedDirs.add(path.dirname(changedFile))
  }
  return changedDirs
}
