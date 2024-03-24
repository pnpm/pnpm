import fs from 'node:fs'
import path from 'node:path'

const dirs = new Set<string>()

export function writeFile(fileDest: string, buffer: Buffer, mode?: number | undefined): void {
  makeDirForFile(fileDest)

  fs.writeFileSync(fileDest, buffer, { mode })
}

function makeDirForFile(fileDest: string): void {
  const dir = path.dirname(fileDest)

  if (!dirs.has(dir)) {
    fs.mkdirSync(dir, { recursive: true })

    dirs.add(dir)
  }
}
