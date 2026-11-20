import fs from 'node:fs'
import path from 'node:path'

import { PnpmError } from '@pnpm/error'

// Characters that Win32 rejects in filenames, plus NUL. Path separators are
// checked separately via `path.basename` so the message is crisp.
const INVALID_FILENAME_CHARS = /[<>:"|?*\0]/
// Win32 reserved device names (case-insensitive, with or without an extension).
const RESERVED_WINDOWS_NAME = /^(?:con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\..*)?$/i

// Reject anything that would let the output escape its target directory, or
// that would fail filesystem-level validation on any supported host. This
// surfaces problems at `pack-app` invocation time instead of letting them
// blow up later in `writeFile(outputFile, ...)`.
export function validateOutputName (name: string): string {
  const hasPathSeparator = name !== path.basename(name) || name.includes('/') || name.includes('\\')
  const isEmptyOrDotName = name === '' || name === '.' || name === '..'
  if (
    hasPathSeparator ||
    isEmptyOrDotName ||
    INVALID_FILENAME_CHARS.test(name) ||
    RESERVED_WINDOWS_NAME.test(name) ||
    /[. ]$/.test(name)
  ) {
    throw new PnpmError('PACK_APP_INVALID_OUTPUT_NAME',
      `Invalid --output-name "${name}". The name must be a plain filename without path separators, Windows-reserved names (e.g. CON, NUL), characters like <>:"|?* or NUL, and must not end in a dot or space.`)
  }
  return name
}

// `entry` and `outputDir` can come from a repo-controlled package.json, so a
// malicious repo must not be able to read/write outside the project via an
// absolute path or `..` traversal. An absolute path replaces the base on join;
// a `..` segment climbs out. Checked lexically before any filesystem access so
// the failure is fast and side-effect-free.
export function pathEscapesProject (rawPath: string): boolean {
  // `path.parse().root` is non-empty for any host-rooted form: a POSIX
  // absolute path (`/x`), and on Windows also the drive-relative (`C:x`) and
  // root-relative (`\x`) forms that `path.isAbsolute()` reports as relative
  // yet still resolve outside the project.
  if (path.parse(rawPath).root !== '') return true
  return rawPath.split(/[/\\]/).includes('..')
}

// Defense in depth beyond `pathEscapesProject`: resolve symlinks and require
// the real target to stay inside the (also symlink-resolved) project dir, so a
// same-name symlink pointing out of the project is caught even though the
// lexical join looked contained. Fails closed on any realpath error.
export function isWithinDir (target: string, dir: string): boolean {
  let realDir: string
  let realTarget: string
  try {
    realDir = fs.realpathSync(dir)
    realTarget = fs.realpathSync(target)
  } catch {
    return false
  }
  const rel = path.relative(realDir, realTarget)
  return rel === '' || (rel !== '..' && !rel.startsWith(`..${path.sep}`) && !path.isAbsolute(rel))
}

// The on-disk file name of the produced executable for a target: a bare name
// on POSIX, suffixed with `.exe` on Windows.
export function outputFileName (outputName: string, platform: string): string {
  return platform === 'win32' ? `${outputName}.exe` : outputName
}

// Refuse to write to `outputFile` when it already exists and is not a regular
// file — most importantly a symlink, which `node --build-sea` would follow to
// overwrite a file outside the project. `lstatSync` does not traverse the final
// component, so a symlink reports `isFile() === false`. A missing path is fine.
export function rejectNonRegularOutputFile (outputFile: string): void {
  const existing = fs.lstatSync(outputFile, { throwIfNoEntry: false })
  if (existing && !existing.isFile()) {
    throw new PnpmError('PACK_APP_OUTPUT_FILE_NOT_REGULAR',
      `The output file "${outputFile}" already exists and is not a regular file (e.g. a symlink); refusing to write through it.`,
      { hint: 'Remove the existing path, or choose a different --output-name or --output-dir.' })
  }
}

export function entryOutsideProjectError (entryPath: string): PnpmError {
  return new PnpmError('PACK_APP_ENTRY_OUTSIDE_PROJECT',
    `The entry path "${entryPath}" resolves outside the project directory.`,
    { hint: 'The entry must be a relative path inside the project directory, not an absolute path or one that escapes via "..".' })
}

export function outputDirOutsideProjectError (outputDir: string): PnpmError {
  return new PnpmError('PACK_APP_OUTPUT_DIR_OUTSIDE_PROJECT',
    `The output directory "${outputDir}" resolves outside the project directory.`,
    { hint: 'The output directory must be a relative path inside the project directory, not an absolute path or one that escapes via "..".' })
}
