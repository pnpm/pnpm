import { type Hunk, type HunkPart, type PatchFilePart, verifyHunkIntegrity } from './parsePatch.js'

/** The patch that undoes `patch`. */
export function reversePatch (patch: PatchFilePart[]): PatchFilePart[] {
  return patch.map(reversePatchPart).reverse()
}

function reversePatchPart (part: PatchFilePart): PatchFilePart {
  switch (part.type) {
    case 'file creation':
      return { type: 'file deletion', path: part.path, hash: part.hash, hunk: part.hunk && reverseHunk(part.hunk), mode: part.mode }
    case 'file deletion':
      return { type: 'file creation', path: part.path, hash: part.hash, hunk: part.hunk && reverseHunk(part.hunk), mode: part.mode }
    case 'rename':
      return { type: 'rename', fromPath: part.toPath, toPath: part.fromPath }
    case 'patch':
      return { type: 'patch', path: part.path, hunks: part.hunks.map(reverseHunk), beforeHash: part.afterHash, afterHash: part.beforeHash }
    case 'mode change':
      return { type: 'mode change', path: part.path, newMode: part.oldMode, oldMode: part.newMode }
  }
}

function reverseHunk (hunk: Hunk): Hunk {
  const parts = hunk.parts.map(reverseHunkPart)
  // Deletions come before the insertions that replace them.
  for (let index = 0; index < parts.length - 1; index++) {
    if (parts[index].type === 'insertion' && parts[index + 1].type === 'deletion') {
      [parts[index], parts[index + 1]] = [parts[index + 1], parts[index]]
      index++
    }
  }
  const reversed: Hunk = {
    header: { original: hunk.header.patched, patched: hunk.header.original },
    parts,
  }
  verifyHunkIntegrity(reversed)
  return reversed
}

function reverseHunkPart (part: HunkPart): HunkPart {
  switch (part.type) {
    case 'context':
      return part
    case 'deletion':
      return { type: 'insertion', lines: part.lines, noNewlineAtEndOfFile: part.noNewlineAtEndOfFile }
    case 'insertion':
      return { type: 'deletion', lines: part.lines, noNewlineAtEndOfFile: part.noNewlineAtEndOfFile }
  }
}
