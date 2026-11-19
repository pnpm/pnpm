import { redactUrlCredentials } from '@pnpm/error'
import { sanitizeInline } from '@pnpm/text.sanitize'

/**
 * Strips control and formatting characters, then inline URL credentials,
 * including those of a protocol-relative `//user:pass@host` value. A URL
 * loses its whitespace first, since whitespace would end the credential scan
 * before the `@`.
 */
export function sanitizeForLog (text: string): string {
  const inline = sanitizeInline(text)
  if (!inline.includes('//')) return redactUrlCredentials(inline)
  const url = inline.replace(/\s/gu, '')
  return url.startsWith('//')
    ? redactUrlCredentials(`x:${url}`).slice(2)
    : redactUrlCredentials(url)
}
