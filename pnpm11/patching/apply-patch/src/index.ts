import fs from 'node:fs'
import path from 'node:path'

import { isError, PnpmError } from '@pnpm/error'

import { executeEffects } from './executeEffects.js'
import { parsePatchFile, type PatchFilePart } from './parsePatch.js'
import { reversePatch } from './reversePatch.js'

export interface ApplyPatchToDirOpts {
  patchedDir: string
  patchFilePath: string
}

export function applyPatchToDir (opts: ApplyPatchToDirOpts): boolean {
  // Ideally, we would run "patch" or "git apply".
  // However, "patch" is not available on Windows and "git apply" is hard to execute on a subdirectory of an existing repository
  const effects = readPatchFile(opts.patchFilePath)
  assertPatchPathsStayInside(opts, effects)
  if (!applyOrConfirmApplied(effects, opts.patchedDir)) {
    throw new PnpmError('PATCH_FAILED', `Could not apply patch ${opts.patchFilePath} to ${opts.patchedDir}`)
  }
  return true
}

function readPatchFile (patchFilePath: string): PatchFilePart[] {
  const patchContent = readPatchContent(patchFilePath)
  try {
    return parsePatchFile(patchContent)
  } catch (err) {
    const message = isError(err) ? err.message : String(err)
    throw new PnpmError('INVALID_PATCH', `Applying patch "${patchFilePath}" failed: ${message}`)
  }
}

function readPatchContent (patchFilePath: string): string {
  try {
    return fs.readFileSync(patchFilePath, 'utf8')
  } catch (err) {
    if (isError(err) && 'code' in err && err.code === 'ENOENT') {
      throw new PnpmError('PATCH_NOT_FOUND', `Patch file not found: ${patchFilePath}`)
    }
    throw err
  }
}

/**
 * Apply the patch to `root`, or confirm that it is already applied there:
 * a patch is taken as applied when its reverse would apply cleanly.
 */
function applyOrConfirmApplied (effects: PatchFilePart[], root: string): boolean {
  try {
    executeEffects(effects, { root, dryRun: false })
    return true
  } catch {
    try {
      executeEffects(reversePatch(effects), { root, dryRun: true })
      return true
    } catch {
      return false
    }
  }
}

// A patch file is attacker-controlled data: `diff --git a/../../X b/../../X` headers
// would otherwise let the applier traverse out of the package directory and write,
// delete, or rename files anywhere the install user can.
function assertPatchPathsStayInside (opts: ApplyPatchToDirOpts, effects: PatchFilePart[]): void {
  const root = path.resolve(opts.patchedDir)
  const rootWithSep = root.endsWith(path.sep) ? root : root + path.sep
  for (const effect of effects) {
    const candidates = effect.type === 'rename'
      ? [effect.fromPath, effect.toPath]
      : [effect.path]
    for (const candidate of candidates) {
      assertCandidatePathStaysInside(opts, candidate, root, rootWithSep)
    }
  }
}

function assertCandidatePathStaysInside (
  opts: ApplyPatchToDirOpts,
  candidate: string,
  root: string,
  rootWithSep: string
): void {
  if (path.isAbsolute(candidate) || candidate.split(/[/\\]/).includes('..')) {
    throw new PatchPathEscapesError(opts, candidate)
  }
  const resolved = path.resolve(root, candidate)
  if (resolved !== root && !resolved.startsWith(rootWithSep)) {
    throw new PatchPathEscapesError(opts, candidate)
  }
}

export class PatchPathEscapesError extends PnpmError {
  constructor (opts: ApplyPatchToDirOpts, badPath: string) {
    super('PATCH_FAILED',
      `Could not apply patch ${opts.patchFilePath} to ${opts.patchedDir}: ` +
      `patch path escapes target dir: ${badPath}`)
  }
}
