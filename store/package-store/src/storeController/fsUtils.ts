import { type Dirent, promises as fs } from 'node:fs'
import util from 'node:util'

export async function getSubdirsSafely (dir: string): Promise<string[]> {
  let entries: Dirent[]
  try {
    entries = await fs.readdir(dir, { withFileTypes: true }) as Dirent[]
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      return []
    }
    throw err
  }
  const subdirs: string[] = []
  for (const entry of entries) {
    if (entry.isDirectory()) {
      subdirs.push(entry.name)
    }
  }
  return subdirs
}
