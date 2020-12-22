import PnpmError from '@pnpm/error'
import * as micromatch from 'micromatch'
import path = require('path')
import execa = require('execa')
import findUp = require('find-up')

type ChangeType = 'source' | 'test'

interface ChangedDir { dir: string, changeType: ChangeType }

export default async function changedSince (packageDirs: string[], commit: string, opts: { workspaceDir: string, testPattern?: string[] }): Promise<[string[], string[]]> {
  const repoRoot = path.resolve(await findUp('.git', { cwd: opts.workspaceDir, type: 'directory' }) ?? opts.workspaceDir, '..')
  const changedDirs = (await getChangedDirsSinceCommit(commit, opts.workspaceDir, opts.testPattern ?? []))
    .map(changedDir => ({ ...changedDir, dir: path.join(repoRoot, changedDir.dir) }))
  const pkgChangeTypes = new Map<string, ChangeType | undefined>()
  for (const pkgDir of packageDirs) {
    pkgChangeTypes.set(pkgDir, undefined)
  }
  for (const changedDir of changedDirs) {
    let currentDir = changedDir.dir
    while (!pkgChangeTypes.has(currentDir)) {
      const nextDir = path.dirname(currentDir)
      if (nextDir === currentDir) break
      currentDir = nextDir
    }
    if (pkgChangeTypes.get(currentDir) === 'source') continue
    pkgChangeTypes.set(currentDir, changedDir.changeType)
  }

  const changedPkgs = [] as string[]
  const ignoreDependentForPkgs = [] as string[]
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
  return [
    [...changedPkgs, ...ignoreDependentForPkgs],
    ignoreDependentForPkgs,
  ]
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
  const changedDirs = new Map<string, ChangeType>()

  if (!diff) {
    return []
  }

  const changedFiles = diff.split('\n')

  for (const changedFile of changedFiles) {
    const dir = path.dirname(changedFile)

    if (changedDirs.get(dir) === 'source') continue

    const changeType: ChangeType = testPattern.some(pattern => micromatch.isMatch(changedFile, pattern))
      ? 'test' : 'source'
    changedDirs.set(dir, changeType)
  }

  return Array.from(changedDirs.entries()).map(([dir, changeType]) => ({ dir, changeType }))
}
