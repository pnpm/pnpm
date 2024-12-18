import fs from 'fs'
import path from 'path'

// The testing folder should be outside of the project to avoid lookup in the project's node_modules
// Not using the OS temp directory due to issues on Windows CI.
const tmpBaseDir = path.join(__dirname, '../../../../pnpm_tmp')

const tmpPath = path.join(tmpBaseDir, `${getFilesCountInDir(tmpBaseDir).toString()}_${process.pid.toString()}`)

let dirNumber = 0

export function tempDir (chdir: boolean = true): string {
  dirNumber++
  const dirname = dirNumber.toString()
  const tmpDir = path.join(tmpPath, dirname)
  fs.mkdirSync(tmpDir, { recursive: true })

  if (chdir) process.chdir(tmpDir)

  return tmpDir
}

function getFilesCountInDir (dir: string): number {
  try {
    return fs.readdirSync(dir).length
  } catch {
    return 0
  }
}
