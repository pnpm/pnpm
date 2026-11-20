import fs from 'node:fs'

const CHUNK_SIZE = 64 * 1024

/**
 * Whether two paths hold the same bytes.
 *
 * Compared rather than hashed: the answer is usually "no" at the first
 * differing byte, and there is nothing to gain from reading further.
 *
 * `candidate` is the untrusted side — a path something else may hold — so
 * anything other than a readable regular file of the same size is not
 * the same file, rather than an error.
 */
export function sameFileContents (file: string, candidate: string): boolean {
  const size = fs.statSync(file).size
  const candidateStat = fs.lstatSync(candidate, { throwIfNoEntry: false })
  if (candidateStat?.isFile() !== true || candidateStat.size !== size) return false

  let fileFd: number | undefined
  let candidateFd: number | undefined
  try {
    fileFd = fs.openSync(file, 'r')
    candidateFd = fs.openSync(candidate, 'r')
    return sameBytes(fileFd, candidateFd)
  } catch {
    return false
  } finally {
    if (fileFd !== undefined) fs.closeSync(fileFd)
    if (candidateFd !== undefined) fs.closeSync(candidateFd)
  }
}

/** Whether the rest of two open files is the same, read chunk by chunk. */
function sameBytes (fdA: number, fdB: number): boolean {
  const bufferA = Buffer.alloc(CHUNK_SIZE)
  const bufferB = Buffer.alloc(CHUNK_SIZE)
  while (true) {
    const readA = fs.readSync(fdA, bufferA, 0, CHUNK_SIZE, null)
    const readB = fs.readSync(fdB, bufferB, 0, CHUNK_SIZE, null)
    if (readA !== readB) return false
    if (readA === 0) return true
    if (!bufferA.subarray(0, readA).equals(bufferB.subarray(0, readB))) return false
  }
}
