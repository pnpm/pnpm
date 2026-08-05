import { types as allTypes } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import npa from '@pnpm/npm-package-arg'
import { pick } from 'ramda'

export function rcOptionsTypes (): Record<string, unknown> {
  return pick([
    'registry',
  ], allTypes)
}

export function parsePackageSpec (spec: string): { name: string, escapedName: string, versionRange: string | undefined } {
  let parsed: ReturnType<typeof npa>
  try {
    parsed = npa(spec)
  } catch {
    throw new PnpmError('INVALID_PACKAGE_SPEC', `Invalid package spec: ${spec}`)
  }
  if (!parsed.name || !parsed.escapedName) {
    throw new PnpmError('INVALID_PACKAGE_SPEC', `Invalid package spec: ${spec}`)
  }
  const versionRange = parsed.rawSpec || undefined
  return { name: parsed.name, escapedName: parsed.escapedName, versionRange }
}

export function normalizeRegistryUrl (registryUrl: string): string {
  return registryUrl.endsWith('/') ? registryUrl : `${registryUrl}/`
}

const ERROR_BODY_LIMIT = 64 * 1024

/**
 * Reads at most ERROR_BODY_LIMIT bytes of the response body. Registry
 * responses are untrusted, so the read is bounded before decoding rather
 * than truncated after buffering the whole body.
 */
export async function readErrorBody (response: Response): Promise<string> {
  const reader = response.body?.getReader()
  if (reader == null) return ''
  const chunks: Uint8Array[] = []
  let total = 0
  let truncated = false
  while (total < ERROR_BODY_LIMIT) {
    // eslint-disable-next-line no-await-in-loop
    const { done, value } = await reader.read()
    if (done) break
    const need = ERROR_BODY_LIMIT - total
    if (value.length > need) {
      chunks.push(value.subarray(0, need))
      truncated = true
      break
    }
    chunks.push(value)
    total += value.length
  }
  reader.cancel().catch(() => {})
  let body = new TextDecoder().decode(Buffer.concat(chunks))
  if (truncated) {
    if (body.length > 0 && !body.endsWith(' ')) {
      body += ' '
    }
    body += '(response body truncated)'
  }
  return body
}
