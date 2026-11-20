import { isError } from '@pnpm/error'

export interface HunkHeader {
  original: { start: number, length: number }
  patched: { start: number, length: number }
}

const NON_EXECUTABLE_FILE_MODE = 0o644
const EXECUTABLE_FILE_MODE = 0o755

export type FileMode = typeof NON_EXECUTABLE_FILE_MODE | typeof EXECUTABLE_FILE_MODE

export type HunkPartType = 'context' | 'insertion' | 'deletion'

export interface HunkPart {
  type: HunkPartType
  lines: string[]
  /** The part's last line ends the file without a final newline. */
  noNewlineAtEndOfFile: boolean
}

export interface Hunk {
  header: HunkHeader
  parts: HunkPart[]
}

export interface FilePatch {
  type: 'patch'
  path: string
  hunks: Hunk[]
  beforeHash: string | null
  afterHash: string | null
}

export interface FileDeletion {
  type: 'file deletion'
  path: string
  mode: FileMode
  hunk: Hunk | null
  hash: string | null
}

export interface FileCreation {
  type: 'file creation'
  path: string
  mode: FileMode
  hunk: Hunk | null
  hash: string | null
}

export interface FileRename {
  type: 'rename'
  fromPath: string
  toPath: string
}

export interface FileModeChange {
  type: 'mode change'
  path: string
  oldMode: FileMode
  newMode: FileMode
}

export type PatchFilePart = FilePatch | FileDeletion | FileCreation | FileRename | FileModeChange

const HUNK_INTEGRITY_ERROR_MESSAGE = 'hunk header integrity check failed'

/**
 * Parse a unified diff as `git diff` writes it.
 *
 * A patch whose hunk headers do not match their bodies is parsed again as
 * an old-style patch: one without `diff --git` lines, where a `--- a/` line
 * starts the next file.
 */
export function parsePatchFile (file: string): PatchFilePart[] {
  const lines = file.split('\n')
  if (lines.at(-1) === '') {
    lines.pop()
  }
  try {
    return interpretFileHeaders(parsePatchLines(lines, { supportLegacyDiffs: false }))
  } catch (err) {
    if (isError(err) && err.message === HUNK_INTEGRITY_ERROR_MESSAGE) {
      return interpretFileHeaders(parsePatchLines(lines, { supportLegacyDiffs: true }))
    }
    throw err
  }
}

/** Throws when the hunk's lines do not add up to the lengths its header states. */
export function verifyHunkIntegrity (hunk: Hunk): void {
  let originalLength = 0
  let patchedLength = 0
  for (const { type, lines } of hunk.parts) {
    if (type !== 'insertion') originalLength += lines.length
    if (type !== 'deletion') patchedLength += lines.length
  }
  if (originalLength !== hunk.header.original.length || patchedLength !== hunk.header.patched.length) {
    throw new Error(HUNK_INTEGRITY_ERROR_MESSAGE)
  }
}

interface FileHeader {
  diffLineFromPath: string | null
  diffLineToPath: string | null
  oldMode: string | null
  newMode: string | null
  deletedFileMode: string | null
  newFileMode: string | null
  renameFrom: string | null
  renameTo: string | null
  beforeHash: string | null
  afterHash: string | null
  fromPath: string | null
  toPath: string | null
  hunks: Hunk[] | null
}

type HeaderValueField = 'oldMode' | 'newMode' | 'deletedFileMode' | 'newFileMode' | 'renameFrom' | 'renameTo'

const HEADER_VALUE_FIELDS: Array<[prefix: string, field: HeaderValueField]> = [
  ['old mode ', 'oldMode'],
  ['new mode ', 'newMode'],
  ['deleted file mode ', 'deletedFileMode'],
  ['new file mode ', 'newFileMode'],
  ['rename from ', 'renameFrom'],
  ['rename to ', 'renameTo'],
]

type HunkLineType = HunkPartType | 'pragma' | 'header'

const HUNK_LINE_TYPES = new Map<string, HunkLineType>([
  ['@', 'header'],
  ['-', 'deletion'],
  ['+', 'insertion'],
  [' ', 'context'],
  ['\\', 'pragma'],
  // A blank line, or one an editor stripped down to its line break, is context.
  ['', 'context'],
  ['\r', 'context'],
])

const NO_NEWLINE_PRAGMA = '\\ No newline at end of file'

interface ParserState {
  readonly files: FileHeader[]
  file: FileHeader
  hunk: Hunk | null
  part: HunkPart | null
  section: 'header' | 'hunks'
  readonly supportLegacyDiffs: boolean
}

function parsePatchLines (lines: string[], opts: { supportLegacyDiffs: boolean }): FileHeader[] {
  const state: ParserState = {
    files: [],
    file: emptyFileHeader(),
    hunk: null,
    part: null,
    section: 'header',
    supportLegacyDiffs: opts.supportLegacyDiffs,
  }
  let index = 0
  while (index < lines.length) {
    const line = lines[index]
    // A step that ends the current section leaves the line for the next one to read.
    const consumed = state.section === 'header' ? readHeaderLine(state, line) : readHunkLine(state, line)
    if (consumed) index++
  }
  commitFile(state)
  for (const { hunks } of state.files) {
    hunks?.forEach(verifyHunkIntegrity)
  }
  return state.files
}

function emptyFileHeader (): FileHeader {
  return {
    diffLineFromPath: null,
    diffLineToPath: null,
    oldMode: null,
    newMode: null,
    deletedFileMode: null,
    newFileMode: null,
    renameFrom: null,
    renameTo: null,
    beforeHash: null,
    afterHash: null,
    fromPath: null,
    toPath: null,
    hunks: null,
  }
}

/** Returns whether the line was consumed. */
function readHeaderLine (state: ParserState, line: string): boolean {
  if (line.startsWith('@@')) {
    state.section = 'hunks'
    state.file.hunks = []
    return false
  }
  if (line.startsWith('diff --git ')) {
    readDiffLine(state, line)
    return true
  }
  const valueField = HEADER_VALUE_FIELDS.find(([prefix]) => line.startsWith(prefix))
  if (valueField != null) {
    const [prefix, field] = valueField
    state.file[field] = line.slice(prefix.length).trim()
  } else if (line.startsWith('index ')) {
    readIndexLine(state.file, line)
  } else if (line.startsWith('--- ')) {
    state.file.fromPath = line.slice('--- a/'.length).trim()
  } else if (line.startsWith('+++ ')) {
    state.file.toPath = line.slice('+++ b/'.length).trim()
  }
  return true
}

function readDiffLine (state: ParserState, line: string): void {
  if (state.file.diffLineFromPath) {
    commitFile(state)
  }
  const paths = parseDiffLinePaths(line)
  if (paths == null) {
    throw new Error(`Bad diff line: ${line}`)
  }
  state.file.diffLineFromPath = paths.fromPath
  state.file.diffLineToPath = paths.toPath
}

const DIFF_LINE_PREFIX = 'diff --git a/'
const DIFF_LINE_SEPARATOR = ' b/'
const LINE_TERMINATORS = ['\r', '\u2028', '\u2029']

/**
 * Split a `diff --git a/<from> b/<to>` line at its first ` b/`, without the
 * trailing whitespace. Neither path may contain a line terminator.
 */
function parseDiffLinePaths (line: string): { fromPath: string, toPath: string } | null {
  if (!line.startsWith(DIFF_LINE_PREFIX)) return null
  const paths = line.slice(DIFF_LINE_PREFIX.length)
  const separatorIndex = paths.indexOf(DIFF_LINE_SEPARATOR)
  if (separatorIndex === -1) return null
  const fromPath = paths.slice(0, separatorIndex)
  const toPath = paths.slice(separatorIndex + DIFF_LINE_SEPARATOR.length).trimEnd()
  if ([fromPath, toPath].some(containsLineTerminator)) return null
  return { fromPath, toPath }
}

function containsLineTerminator (text: string): boolean {
  return LINE_TERMINATORS.some((terminator) => text.includes(terminator))
}

function readIndexLine (file: FileHeader, line: string): void {
  const match = /(\w+)\.\.(\w+)/.exec(line)
  if (match == null) return
  file.beforeHash = match[1]
  file.afterHash = match[2]
}

/** Returns whether the line was consumed. */
function readHunkLine (state: ParserState, line: string): boolean {
  if (state.supportLegacyDiffs && line.startsWith('--- a/')) {
    state.section = 'header'
    commitFile(state)
    return false
  }
  const lineType = HUNK_LINE_TYPES.get(line.slice(0, 1))
  switch (lineType) {
    case undefined:
      state.section = 'header'
      commitFile(state)
      return false
    case 'header':
      commitHunk(state)
      state.hunk = { header: parseHunkHeaderLine(line), parts: [] }
      return true
    case 'pragma':
      readPragmaLine(state, line)
      return true
    default:
      appendHunkLine(state, lineType, line.slice(1))
      return true
  }
}

function readPragmaLine (state: ParserState, line: string): void {
  if (!line.startsWith(NO_NEWLINE_PRAGMA)) {
    throw new Error(`Unrecognized pragma in patch file: ${line}`)
  }
  if (state.part == null) {
    throw new Error('Bad parser state: No newline at EOF pragma encountered without context')
  }
  state.part.noNewlineAtEndOfFile = true
}

function appendHunkLine (state: ParserState, type: HunkPartType, content: string): void {
  if (state.hunk == null) {
    throw new Error('Bad parser state: Hunk lines encountered before hunk header')
  }
  if (state.part != null && state.part.type !== type) {
    state.hunk.parts.push(state.part)
    state.part = null
  }
  state.part ??= { type, lines: [], noNewlineAtEndOfFile: false }
  state.part.lines.push(content)
}

function commitHunk (state: ParserState): void {
  if (state.hunk == null) return
  if (state.part != null) {
    state.hunk.parts.push(state.part)
    state.part = null
  }
  state.file.hunks ??= []
  state.file.hunks.push(state.hunk)
  state.hunk = null
}

function commitFile (state: ParserState): void {
  commitHunk(state)
  state.files.push(state.file)
  state.file = emptyFileHeader()
}

function parseHunkHeaderLine (headerLine: string): HunkHeader {
  const match = /^@@ -(\d+)(,(\d+))? \+(\d+)(,(\d+))? @@.*/.exec(headerLine.trim())
  if (match == null) {
    throw new Error(`Bad header line: '${headerLine}'`)
  }
  return {
    original: { start: Math.max(Number(match[1]), 1), length: Number(match[3] || 1) },
    patched: { start: Math.max(Number(match[4]), 1), length: Number(match[6] || 1) },
  }
}

function interpretFileHeaders (files: FileHeader[]): PatchFilePart[] {
  return files.flatMap(interpretFileHeader)
}

function interpretFileHeader (file: FileHeader): PatchFilePart[] {
  const parts: PatchFilePart[] = []
  const destinationPath = pushFileOperation(parts, file)
  if (destinationPath && file.oldMode && file.newMode && file.oldMode !== file.newMode) {
    parts.push({
      type: 'mode change',
      path: destinationPath,
      oldMode: parseFileMode(file.oldMode),
      newMode: parseFileMode(file.newMode),
    })
  }
  if (destinationPath && file.hunks?.length) {
    parts.push({
      type: 'patch',
      path: destinationPath,
      hunks: file.hunks,
      beforeHash: file.beforeHash,
      afterHash: file.afterHash,
    })
  }
  return parts
}

/**
 * Push the file's creation, deletion, or rename, and return the path that a
 * mode change or content patch of the same file applies to, if any.
 */
function pushFileOperation (parts: PatchFilePart[], file: FileHeader): string | null {
  if (file.renameFrom) {
    if (!file.renameTo) {
      throw new Error('Bad parser state: rename from & to not given')
    }
    parts.push({ type: 'rename', fromPath: file.renameFrom, toPath: file.renameTo })
    return file.renameTo
  }
  if (file.deletedFileMode) {
    parts.push({
      type: 'file deletion',
      path: requirePath(file.diffLineFromPath || file.fromPath, 'file deletion'),
      mode: parseFileMode(file.deletedFileMode),
      hunk: file.hunks?.[0] ?? null,
      hash: file.beforeHash,
    })
    return null
  }
  if (file.newFileMode) {
    parts.push({
      type: 'file creation',
      path: requirePath(file.diffLineToPath || file.toPath, 'file creation'),
      mode: parseFileMode(file.newFileMode),
      hunk: file.hunks?.[0] ?? null,
      hash: file.afterHash,
    })
    return null
  }
  return file.toPath || file.diffLineToPath
}

function requirePath (filePath: string | null, operation: string): string {
  if (!filePath) {
    throw new Error(`Bad parse state: no path given for ${operation}`)
  }
  return filePath
}

function parseFileMode (mode: string): FileMode {

  const permissionBits = parseInt(mode, 8) & 0o777
  if (permissionBits !== NON_EXECUTABLE_FILE_MODE && permissionBits !== EXECUTABLE_FILE_MODE) {
    throw new Error(`Unexpected file mode string: ${mode}`)
  }
  return permissionBits
}
