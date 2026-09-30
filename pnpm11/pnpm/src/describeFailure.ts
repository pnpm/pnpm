import { isError, redactAndSanitize } from '@pnpm/error'

/**
 * The code and message of `err`, made safe to print. A Node.js filesystem
 * error opens its message with the code, so naming it again would repeat it.
 */
export function describeFailure (err: unknown): string {
  if (!isError(err)) return redactAndSanitize(String(err))
  const code = 'code' in err ? String(err.code) : ''
  const described = code === '' || err.message.startsWith(code) ? err.message : `${code}: ${err.message}`
  return redactAndSanitize(described)
}
