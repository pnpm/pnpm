import fs from 'node:fs'
import path from 'node:path'

import { isError } from '@pnpm/error'

const README_RANK = {
  bare: 1,
  markdown: 2,
  readmeMd: 3,
} as const

export interface ReadmeCandidate {
  fileName: string
  rank: ReadmeRank
}

/** How npm ranks a package-root file as the package's README. A higher rank wins. */
export type ReadmeRank = typeof README_RANK[keyof typeof README_RANK]

/**
 * Rank a filename as a README candidate, or return `undefined` if npm would not use it as the
 * package's README. npm matches the bare `README` case-sensitively and accepts `README.*` whose
 * extension matches `/.m?a?r?k?d?o?w?n?$/i`.
 */
export function getReadmeRank (fileName: string): ReadmeRank | undefined {
  if (/^readme\.md$/i.test(fileName)) return README_RANK.readmeMd
  if (fileName === 'README') return README_RANK.bare
  if (/^readme\./i.test(fileName) && /\.m?a?r?k?d?o?w?n?$/i.test(fileName)) return README_RANK.markdown
  return undefined
}

/**
 * Whether README `candidate` should replace the `current` selection. A higher rank wins. Equal
 * ranks keep the lower filename, so the choice does not depend on directory or archive order, and
 * an equal filename replaces, so the last duplicate archive entry wins as it would on extraction.
 */
export function isPreferredReadme (candidate: ReadmeCandidate, current: ReadmeCandidate | undefined): boolean {
  if (current == null || candidate.rank > current.rank) return true
  return candidate.rank === current.rank && candidate.fileName <= current.fileName
}

// `O_NOFOLLOW` makes the open itself refuse a symlink at the final path component, closing the
// TOCTOU window between the `readdir` type check and the read: a symlink swapped in for the
// selected README after the check can't redirect the read outside the project and leak its target
// into the published manifest. Windows lacks the flag (and requires privileges to create symlinks),
// so it falls back to a plain read.
const README_READ_FLAGS = fs.constants.O_RDONLY | (process.platform === 'win32' ? 0 : fs.constants.O_NOFOLLOW)

export async function readReadmeFile (projectDir: string): Promise<string | undefined> {
  const entries = await fs.promises.readdir(projectDir, { withFileTypes: true })
  let readme: ReadmeCandidate | undefined
  for (const entry of entries) {
    const rank = getReadmeRank(entry.name)
    if (rank == null || !entry.isFile()) continue
    const candidate = { fileName: entry.name, rank }
    if (isPreferredReadme(candidate, readme)) readme = candidate
  }
  if (readme == null) return undefined
  return readReadmeContent(projectDir, readme.fileName)
}

async function readReadmeContent (projectDir: string, fileName: string): Promise<string | undefined> {
  let handle: fs.promises.FileHandle | undefined
  try {
    handle = await fs.promises.open(path.join(projectDir, fileName), README_READ_FLAGS)
    return await handle.readFile('utf8')
  } catch (err: unknown) {
    // ELOOP: the entry is a symlink after all (a concurrent swap) — skip it, as the isFile() check intended.
    if (isError(err) && 'code' in err && err.code === 'ELOOP') return undefined
    throw err
  } finally {
    await handle?.close()
  }
}
