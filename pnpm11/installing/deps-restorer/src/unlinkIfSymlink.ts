import { promises as fs } from 'node:fs'

import { isError } from '@pnpm/error'

export async function unlinkIfSymlink (link: string): Promise<void> {
  if (!await isSymlink(link)) return
  // A non-recursive unlink: an entry that stopped being a link since the
  // check is a directory, which unlink refuses to remove.
  try {
    await fs.unlink(link)
  } catch (err: unknown) {
    if (!isError(err) || !('code' in err) || err.code !== 'ENOENT') throw err
  }
}

export async function isSymlink (path: string): Promise<boolean> {
  const stats = await fs.lstat(path).catch((err: unknown) => {
    if (isError(err) && 'code' in err && err.code === 'ENOENT') return undefined
    throw err
  })
  return stats?.isSymbolicLink() === true
}
