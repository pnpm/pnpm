import PnpmError from '@pnpm/error'
import * as micromatch from 'micromatch'
import path = require('path')
import execa = require('execa')
import findUp = require('find-up')
import isSubdir = require('is-subdir')

type ChangeType = 'source' | 'test'

interface ChangedDir { dir: string, changeType: ChangeType }

export default async function changedSince (packageDirs: string[], commit: string, opts: { workspaceDir: string, testPattern?: string[] }): Promise<[string[], string[]]> {
  const repoRoot = path.resolve(await findUp('.git', { cwd: opts.workspaceDir, type: 'directory' }) ?? opts.workspaceDir, '..')
  const changedDirs = (await getChangedDirsSinceCommit(commit, opts.workspaceDir, opts.testPattern ?? []))
    .map(changedDir => ({ ...changedDir, dir: path.join(repoRoot, changedDir.dir) }))
  let changedSourceDirs = changedDirs.filter(changedDir => changedDir.changeType === 'source')
  const changedPkgs: string[] = []
  for (const packageDir of packageDirs.sort((pkgDir1, pkgDir2) => pkgDir2.length - pkgDir1.length)) {
    if (
      changedSourceDirs.some(changedDir => isSubdir(packageDir, changedDir.dir))
    ) {
      changedSourceDirs = changedSourceDirs.filter((changedDir) => !isSubdir(packageDir, changedDir.dir))
      changedPkgs.push(packageDir)
    }
  }

  const ignoreDependentForPkgs = changedDirs.filter(changedDir => changedDir.changeType === 'test')
    .filter(changedDir => changedPkgs.find(pkg => changedDir.dir.startsWith(pkg)))
    .map(changedDir => changedDir.dir)

  return [changedPkgs, ignoreDependentForPkgs]
}

async function getChangedDirsSinceCommit (commit: string, workingDir: string, testPattern: string[]): Promise<ChangedDir[]> {
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
  const changedDirs = new Set<ChangedDir>()
  const dirsMatchingFilter = new Set<ChangedDir>()

  if (!diff) {
    return []
  }

  const changedFiles = diff.split('\n')

  for (const changedFile of changedFiles) {
    const dirName = path.dirname(changedFile)

    changedDirs.add({ dir: dirName, changeType: 'source' })

    if (testPattern.some(pattern => micromatch.isMatch(changedFile, pattern))) {
      dirsMatchingFilter.add({ dir: dirName, changeType: 'test' })
    }
  }

  return [...Array.from(changedDirs), ...Array.from(dirsMatchingFilter)]
}
