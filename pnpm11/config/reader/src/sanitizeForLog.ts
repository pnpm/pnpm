import { redactUrlCredentials } from '@pnpm/error'
import { sanitizeInline } from '@pnpm/text.sanitize'

/**
 * Strips control and formatting characters, then inline URL credentials,
 * including those of a protocol-relative `//user:pass@host` value.
 */
export function sanitizeForLog (text: string): string {
  const inline = sanitizeInline(text)
  const trimmed = inline.trimStart()
  return trimmed.startsWith('//')
    ? redactUrlCredentials(`x:${trimmed}`).slice(2)
    : redactUrlCredentials(inline)
}
