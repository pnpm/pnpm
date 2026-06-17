import { execFileSync } from 'node:child_process'
import path from 'node:path'

import { globalWarn } from '@pnpm/logger'

const QUARANTINE_ATTR = 'com.apple.quarantine'
// Quarantine removal only ever runs on macOS, so the set lists the binary
// formats Gatekeeper guards there (.dll is Windows-only and never matches).
const NATIVE_BINARY_EXTENSIONS = new Set(['.node', '.dylib', '.so'])
// Cap the bytes of file-path arguments per `xattr` call so a package with many
// (or very long) native-binary paths can't blow past the OS argv limit
// (ARG_MAX, ~1 MB on macOS). Well under the limit to leave room for argv0 and
// the environment block.
const MAX_ARG_BYTES = 100_000

/**
 * Native binaries are the only files that macOS Gatekeeper blocks for carrying a
 * quarantine xattr, so removing it from anything else (JavaScript, text, etc.)
 * just wastes a syscall.
 */
export function isNativeBinary (filePath: string): boolean {
  return NATIVE_BINARY_EXTENSIONS.has(path.extname(filePath).toLowerCase())
}

/**
 * Remove the macOS Gatekeeper quarantine xattr (com.apple.quarantine) from the
 * given files using a single `xattr` invocation.
 *
 * macOS preserves extended attributes when pnpm copies or reflinks files out of
 * its content-addressable store. If a store blob carries com.apple.quarantine
 * (e.g. it was first written under a Gatekeeper-enabled app such as a Git
 * client), the quarantine propagates to node_modules and Gatekeeper blocks the
 * native binary from loading, even though pnpm already verified the file's
 * integrity against the lockfile. This mirrors Homebrew's behaviour of dropping
 * quarantine from verified downloads.
 *
 * File paths are passed as separate arguments rather than interpolated into a
 * shell command, so package-controlled filenames cannot inject shell commands.
 * They are split into chunks that stay under the OS argv limit.
 */
export function removeQuarantine (filePaths: string[]): void {
  if (process.platform !== 'darwin') return
  for (const chunk of chunkByArgSize(filePaths)) {
    removeQuarantineFromChunk(chunk)
  }
}

function removeQuarantineFromChunk (filePaths: string[]): void {
  try {
    execFileSync('/usr/bin/xattr', ['-d', QUARANTINE_ATTR, ...filePaths], {
      stdio: ['ignore', 'ignore', 'pipe'],
    })
  } catch (err: unknown) {
    // `xattr -d` exits non-zero when a file simply has no quarantine xattr to
    // remove ("No such xattr"), which is the common, expected case. It also
    // reports "No such file" for entries the importer legitimately dropped or
    // renamed (e.g. case-insensitive filename conflicts). Surface only errors
    // that are not of those kinds (e.g. permission denied).
    const realErrors = getStderr(err)
      .split('\n')
      .filter((line) => line.trim() !== '' && !line.includes('No such xattr') && !line.includes('No such file'))
    if (realErrors.length > 0) {
      globalWarn(`Failed to remove the macOS quarantine attribute:\n${realErrors.join('\n')}`)
    }
  }
}

function chunkByArgSize (filePaths: string[]): string[][] {
  const chunks: string[][] = []
  let chunk: string[] = []
  let chunkBytes = 0
  for (const filePath of filePaths) {
    const bytes = Buffer.byteLength(filePath) + 1 // +1 for the argv null terminator
    if (chunk.length > 0 && chunkBytes + bytes > MAX_ARG_BYTES) {
      chunks.push(chunk)
      chunk = []
      chunkBytes = 0
    }
    chunk.push(filePath)
    chunkBytes += bytes
  }
  if (chunk.length > 0) chunks.push(chunk)
  return chunks
}

function getStderr (err: unknown): string {
  if (typeof err === 'object' && err !== null && 'stderr' in err) {
    const stderr = (err as { stderr?: Buffer | string }).stderr
    if (stderr != null) return stderr.toString()
  }
  return err instanceof Error ? err.message : String(err)
}
