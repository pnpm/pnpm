import '@total-typescript/ts-reset'

import { PnpmError } from '@pnpm/error'
import type { ApplyPatchToDirOpts } from '@pnpm/types'
import { applyPatch } from '@pnpm/patch-package/dist/applyPatches'

export function applyPatchToDir(opts: ApplyPatchToDirOpts) {
  // Ideally, we would just run "patch" or "git apply".
  // However, "patch" is not available on Windows and "git apply" is hard to execute on a subdirectory of an existing repository
  const cwd = process.cwd()

  process.chdir(opts.patchedDir)

  let success = false

  try {
    success = applyPatch({
      patchFilePath: opts.patchFilePath,
    })
  } catch (err: unknown) {
    // @ts-ignore
    if (err.code === 'ENOENT') {
      throw new PnpmError(
        'PATCH_NOT_FOUND',
        `Patch file not found: ${opts.patchFilePath}`
      )
    }

    throw new PnpmError(
      'INVALID_PATCH',
      // @ts-ignore
      `Applying patch "${opts.patchFilePath}" failed: ${err.message as string}`
    )
  } finally {
    process.chdir(cwd)
  }
  if (!success) {
    throw new PnpmError(
      'PATCH_FAILED',
      `Could not apply patch ${opts.patchFilePath} to ${opts.patchedDir}`
    )
  }
}
