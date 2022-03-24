import { promises as fs } from 'fs'
import path from 'path'

export async function cleanGitBranchLockfiles (pkgPath: string) {
  const files = await fs.readdir(pkgPath)
  const gitBranchLockfiles: string[] = files.filter(file => file.match(/pnpm-lock.(?:.*).yaml/))
  await Promise.all(
    gitBranchLockfiles.map(async file => {
      const filepath: string = path.join(pkgPath, file)
      await fs.rm(filepath)
    })
  )
}