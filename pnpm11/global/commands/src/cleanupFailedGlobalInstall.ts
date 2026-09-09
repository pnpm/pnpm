import fs from 'node:fs'

import { getSingleLineErrorMessage } from './errorMessage.js'

/**
 * Discard the fresh install directory after a step that runs before
 * activation failed, then rethrow that failure. Nothing outside the
 * directory has been touched yet, so removing it leaves the global
 * installation exactly as the command found it.
 *
 * A removal that fails is reported alongside the original failure, which
 * stays the aggregate's `cause`: the directory it left behind is the only
 * trace of the aborted install, so the user has to hear about it.
 */
export async function cleanupFailedGlobalInstall (
  installDir: string,
  originalError: unknown
): Promise<never> {
  try {
    await fs.promises.rm(installDir, { recursive: true, force: true })
  } catch (cleanupError) {
    throw new AggregateError(
      [originalError, cleanupError],
      'Failed to clean up after global install failed before activation. ' +
        `Original error: ${getSingleLineErrorMessage(originalError)}. ` +
        `Cleanup error: ${getSingleLineErrorMessage(cleanupError)}.`,
      { cause: originalError } // eslint-disable-line preserve-caught-error -- The failure before activation is primary; both errors remain in AggregateError.errors.
    )
  }
  throw originalError
}
