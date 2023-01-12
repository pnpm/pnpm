import { PnpmError } from '@pnpm/error'
import { applyPatch } from 'patch-package/dist/applyPatches'

export interface ApplyPatchToDirOpts {
  patchedDir: string
  patchFilePath: string
}

export function applyPatchToDir (opts: ApplyPatchToDirOpts) {
  // Ideally, we would just run "patch" or "git apply".
  // However, "patch" is not available on Windows and "git apply" is hard to execute on a subdirectory of an existing repository
  const cwd = process.cwd()
  process.chdir(opts.patchedDir)
  const success = applyPatch({
    patchDir: opts.patchedDir,
    patchFilePath: opts.patchFilePath,
  })
  process.chdir(cwd)
  if (!success) {
    throw new PnpmError('PATCH_FAILED', `Could not apply patch ${opts.patchFilePath} to ${opts.patchedDir}`)
  }
}
