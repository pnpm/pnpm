import fs from 'node:fs'
import util from 'node:util'

import { getSingleLineErrorMessage } from './errorMessage.js'

/**
 * Discard the fresh install directory after a step that runs before
 * activation failed, then rethrow that failure. Nothing outside the
 * directory has been touched yet, so removing it leaves the global
 * installation exactly as the command found it.
 *
 * A removal that fails is reported alongside the original failure, which
 * stays the aggregate's `cause` and lends it its `code`: the directory it
 * left behind is the only trace of the aborted install, so the user has to
 * hear about it, but the install still failed for the reason it reports.
 */
export async function cleanupFailedGlobalInstall (
  installDir: string,
  originalError: unknown
): Promise<never> {
  try {
    await fs.promises.rm(installDir, { recursive: true, force: true })
  } catch (cleanupError) {
    const failure: AggregateError & { code?: string } = new AggregateError(
      [originalError, cleanupError],
      'Failed to clean up after global install failed before activation. ' +
        `Original error: ${getSingleLineErrorMessage(originalError)}. ` +
        `Cleanup error: ${getSingleLineErrorMessage(cleanupError)}.`,
      { cause: originalError }
    )
    // The reporter and the parseable error output read `code` off the error
    // they are handed, so without this the install that actually failed would
    // be rendered as a generic error.
    const code = getErrorCode(originalError)
    if (code != null) failure.code = code
    throw failure
  }
  throw originalError
}

function getErrorCode (err: unknown): string | undefined {
  if (!util.types.isNativeError(err) || !('code' in err)) return undefined
  return typeof err.code === 'string' ? err.code : undefined
}
