import util from 'node:util'

import { redactAndSanitize } from '@pnpm/error'

export function getErrorMessage (err: unknown): string {
  if (util.types.isNativeError(err)) return err.message
  try {
    return String(err)
  } catch {
    return 'Unknown error'
  }
}

/**
 * The message of `err`, safe to embed in a message of our own: credentials
 * are redacted and every separator a terminal or a log line would act on is
 * stripped, the Unicode line and paragraph separators included.
 */
export function getSingleLineErrorMessage (err: unknown): string {
  return redactAndSanitize(getErrorMessage(err))
    .replaceAll('\u2028', '')
    .replaceAll('\u2029', '')
}
