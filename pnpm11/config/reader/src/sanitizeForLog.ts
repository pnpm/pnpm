import { redactUrlCredentials } from '@pnpm/error'
import { sanitizeInline } from '@pnpm/text.sanitize'

/** Strips control and formatting characters, then inline URL credentials. */
export function sanitizeForLog (text: string): string {
  return redactUrlCredentials(sanitizeInline(text))
}
