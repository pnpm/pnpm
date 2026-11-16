import fs from 'node:fs'

import type { FilePatch, Hunk, HunkPart } from './parsePatch.js'

/** How far either side of its recorded position a hunk is retried. */
const MAX_FUZZING_OFFSET = 20

/**
 * Apply the hunks of a content patch to the file at `filePath`.
 *
 * - Lines are compared with trailing whitespace stripped, so an LF patch
 *   applies to a CRLF file.
 * - A hunk that does not match at its recorded position is retried up to
 *   {@link MAX_FUZZING_OFFSET} lines either side of it.
 * - Every hunk is located against the unpatched file before any is applied.
 *
 * Throws when a hunk matches nowhere. With `dryRun`, the file is left as is.
 */
export function applyHunks (filePatch: FilePatch, filePath: string, dryRun: boolean): void {
  const mode = fs.statSync(filePath).mode
  const fileLines = fs.readFileSync(filePath, 'utf8').split('\n')
  const modifications = filePatch.hunks.flatMap((hunk, hunkIndex) => {
    const found = findHunk(hunk, fileLines)
    if (found == null) {
      throw new Error(`Cannot apply hunk ${hunkIndex} for file ${filePatch.path}`)
    }
    return found
  })
  if (dryRun) return
  replayModifications(fileLines, modifications)
  // Writing through a hardlink would change every other link to the file,
  // such as its copy in the store. Unlinking first gives the file a new inode.
  fs.unlinkSync(filePath)
  fs.writeFileSync(filePath, fileLines.join('\n'), { mode })
}

/**
 * An edit to the file's lines, recorded while matching and replayed
 * afterwards. `push` and `pop` add and remove the trailing empty element
 * that stands for the file's final newline.
 */
type Modification =
  | { type: 'splice', index: number, numToDelete: number, linesToInsert: string[] }
  | { type: 'pop' }
  | { type: 'push' }

function findHunk (hunk: Hunk, fileLines: string[]): Modification[] | null {
  for (const fuzzingOffset of fuzzingOffsets()) {
    const modifications = evaluateHunk(hunk, fileLines, hunk.header.original.start - 1 + fuzzingOffset)
    if (modifications != null) return modifications
  }
  return null
}

/** `0, -1, 1, -2, 2, ...` out to {@link MAX_FUZZING_OFFSET}. */
function * fuzzingOffsets (): Generator<number> {
  yield 0
  for (let offset = 1; offset <= MAX_FUZZING_OFFSET; offset++) {
    yield -offset
    yield offset
  }
}

function evaluateHunk (hunk: Hunk, fileLines: string[], start: number): Modification[] | null {
  if (start < 0 || fileLines.length - start < hunk.header.original.length) return null
  const modifications: Modification[] = []
  let index = start
  let preImageEnd: PreImageEnd = 'elsewhere'
  for (const part of hunk.parts) {
    if (part.type === 'insertion') {
      pushInsertion(modifications, part, index, preImageEnd)
      continue
    }
    const end = matchPreImage(part, fileLines, index)
    if (end == null) return null
    index = end
    preImageEnd = preImageEndOf(part, fileLines, index)
    if (part.type === 'deletion') {
      pushDeletion(modifications, part, index)
    }
  }
  return modifications
}

/**
 * Where the pre-image lines matched so far stop in the file.
 * `unterminatedLastLine`: on the file's last line, which has no final newline,
 * although the hunk carries no "No newline at end of file" marker for it.
 */
type PreImageEnd = 'unterminatedLastLine' | 'elsewhere'

function preImageEndOf (part: HunkPart, fileLines: string[], index: number): PreImageEnd {
  const reachesUnterminatedEnd = !part.noNewlineAtEndOfFile &&
    index === fileLines.length &&
    fileLines.at(-1) !== ''
  return reachesUnterminatedEnd ? 'unterminatedLastLine' : 'elsewhere'
}

/** Returns where the part's lines end in the file, or `null` when they do not match there. */
function matchPreImage (part: HunkPart, fileLines: string[], start: number): number | null {
  let index = start
  for (const line of part.lines) {
    if (fileLines[index].trimEnd() !== line.trimEnd()) return null
    index++
  }
  return index
}

function pushDeletion (modifications: Modification[], part: HunkPart, end: number): void {
  modifications.push({ type: 'splice', index: end - part.lines.length, numToDelete: part.lines.length, linesToInsert: [] })
  // The deleted run ended the file without a newline. Unless an insertion
  // says otherwise, the patched file gets one.
  if (part.noNewlineAtEndOfFile) {
    modifications.push({ type: 'push' })
  }
}

function pushInsertion (modifications: Modification[], part: HunkPart, index: number, preImageEnd: PreImageEnd): void {
  // `git diff --ignore-cr-at-eol`, which `pnpm patch-commit` runs, writes
  // lines appended to a file without a final newline as context followed by
  // the insertion. The newline is restored so that the insertion's "pop"
  // removes it rather than the last inserted line.
  if (part.noNewlineAtEndOfFile && preImageEnd === 'unterminatedLastLine') {
    modifications.push({ type: 'push' })
  }
  modifications.push({ type: 'splice', index, numToDelete: 0, linesToInsert: part.lines })
  if (part.noNewlineAtEndOfFile) {
    modifications.push({ type: 'pop' })
  }
}

function replayModifications (fileLines: string[], modifications: Modification[]): void {
  let offset = 0
  for (const modification of modifications) {
    switch (modification.type) {
      case 'splice':
        fileLines.splice(modification.index + offset, modification.numToDelete, ...modification.linesToInsert)
        offset += modification.linesToInsert.length - modification.numToDelete
        break
      case 'pop':
        fileLines.pop()
        break
      case 'push':
        fileLines.push('')
        break
    }
  }
}
