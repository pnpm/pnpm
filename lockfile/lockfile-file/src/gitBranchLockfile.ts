import { promises as fs } from 'node:fs'
import path from 'node:path'

export async function getGitBranchLockfileNames(lockfileDir: string): Promise<string[]> {
  const files = await fs.readdir(lockfileDir)
  const gitBranchLockfileNames: string[] = files.filter((file) =>
    file.match(/^pnpm-lock.(?:.*).yaml$/)
  )
  return gitBranchLockfileNames
}

export async function cleanGitBranchLockfiles(lockfileDir: string): Promise<void> {
  const gitBranchLockfiles: string[] =
    await getGitBranchLockfileNames(lockfileDir)
  await Promise.all(
    gitBranchLockfiles.map(async (file: string): Promise<void> => {
      const filepath: string = path.join(lockfileDir, file)
      await fs.unlink(filepath)
    })
  )
}
