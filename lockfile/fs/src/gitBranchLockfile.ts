import fs, { promises as fsp } from 'node:fs'
import path from 'node:path'

// eslint-disable-next-line regexp/no-useless-non-capturing-group
const GIT_BRANCH_LOCKFILE_NAME = /^pnpm-lock.(?:.*).yaml$/

export async function getGitBranchLockfileNames (lockfileDir: string): Promise<string[]> {
  const files = await fsp.readdir(lockfileDir)
  return files.filter(file => GIT_BRANCH_LOCKFILE_NAME.test(file))
}

export function getGitBranchLockfileNamesSync (lockfileDir: string): string[] {
  const files = fs.readdirSync(lockfileDir)
  return files.filter(file => GIT_BRANCH_LOCKFILE_NAME.test(file))
}

export async function cleanGitBranchLockfiles (lockfileDir: string): Promise<void> {
  const gitBranchLockfiles: string[] = await getGitBranchLockfileNames(lockfileDir)
  await Promise.all(
    gitBranchLockfiles.map(async file => {
      const filepath: string = path.join(lockfileDir, file)
      await fsp.unlink(filepath)
    })
  )
}
