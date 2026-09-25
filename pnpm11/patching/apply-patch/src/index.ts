import fs from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import { PnpmError } from '@pnpm/error'
import { applyPatch } from '@pnpm/patch-package/dist/applyPatches.js'
import { parsePatchFile } from '@pnpm/patch-package/dist/patch/parse.js'

export interface ApplyPatchToDirOpts {
  patchedDir: string
  patchFilePath: string
}

export function applyPatchToDir (opts: ApplyPatchToDirOpts): boolean {
  // Ideally, we would just run "patch" or "git apply".
  // However, "patch" is not available on Windows and "git apply" is hard to execute on a subdirectory of an existing repository
  assertPatchPathsStayInside(opts)
  const cwd = process.cwd()
  process.chdir(opts.patchedDir)
  let success = false
  try {
    success = applyPatch({
      patchFilePath: opts.patchFilePath,
    })
  } catch (err) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      throw new PnpmError('PATCH_NOT_FOUND', `Patch file not found: ${opts.patchFilePath}`)
    }
    const message = util.types.isNativeError(err) ? err.message : String(err)
    throw new PnpmError('INVALID_PATCH', `Applying patch "${opts.patchFilePath}" failed: ${message}`)
  } finally {
    process.chdir(cwd)
  }
  if (!success) {
    throw new PnpmError('PATCH_FAILED', `Could not apply patch ${opts.patchFilePath} to ${opts.patchedDir}`)
  }
  return success
}

// A patch file is attacker-controlled data: `diff --git a/../../X b/../../X` headers
// would otherwise let the applier traverse out of the package directory and write,
// delete, or rename files anywhere the install user can.
function assertPatchPathsStayInside (opts: ApplyPatchToDirOpts): void {
  let patchContent: string
  try {
    patchContent = fs.readFileSync(opts.patchFilePath, 'utf8')
  } catch (err) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      throw new PnpmError('PATCH_NOT_FOUND', `Patch file not found: ${opts.patchFilePath}`)
    }
    throw err
  }
  let effects
  try {
    effects = parsePatchFile(patchContent)
  } catch {
    // Defer parse-error reporting to applyPatch so its existing
    // ERR_PNPM_INVALID_PATCH path produces the message and exit behavior.
    return
  }
  const root = path.resolve(opts.patchedDir)
  const rootWithSep = root.endsWith(path.sep) ? root : root + path.sep
  for (const effect of effects) {
    const candidates: Array<string | undefined> = effect.type === 'rename'
      ? [effect.fromPath, effect.toPath]
      : [effect.path]
    for (const candidate of candidates) {
      if (!candidate) continue
      if (
        path.isAbsolute(candidate) ||
        candidate.split(/[/\\]/).includes('..')
      ) {
        throw new PatchPathEscapesError(opts, candidate)
      }
      const resolved = path.resolve(root, candidate)
      if (resolved !== root && !resolved.startsWith(rootWithSep)) {
        throw new PatchPathEscapesError(opts, candidate)
      }
    }
  }
}

export class PatchPathEscapesError extends PnpmError {
  constructor (opts: ApplyPatchToDirOpts, badPath: string) {
    super('PATCH_FAILED',
      `Could not apply patch ${opts.patchFilePath} to ${opts.patchedDir}: ` +
      `patch path escapes target dir: ${badPath}`)
  }
}
