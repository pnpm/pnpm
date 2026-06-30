import { WANTED_LOCKFILE } from '@pnpm/constants'

export class PnpmError extends Error {
  public readonly code: string
  public readonly hint?: string
  public attempts?: number
  public prefix?: string
  public pkgsStack?: Array<{ id: string, name: string, version: string }>
  constructor (
    code: string,
    message: string,
    opts?: {
      attempts?: number
      hint?: string
      cause?: unknown
    }
  ) {
    super(message, { cause: opts?.cause })
    this.code = code.startsWith('ERR_PNPM_') ? code : `ERR_PNPM_${code}`
    this.hint = opts?.hint
    this.attempts = opts?.attempts
  }
}

export interface FetchErrorResponse {
  status: number, statusText: string
}

export interface FetchErrorRequest {
  url: string, authHeaderValue?: string
}

export class FetchError extends PnpmError {
  public readonly response: FetchErrorResponse
  public readonly request: FetchErrorRequest

  constructor (
    request: FetchErrorRequest,
    response: FetchErrorResponse,
    hint?: string
  ) {
    const _request: FetchErrorRequest = {
      url: request.url,
    }
    if (request.authHeaderValue) {
      _request.authHeaderValue = hideAuthInformation(request.authHeaderValue)
    }
    const message = `GET ${redactUrlCredentials(request.url)}: ${response.statusText} - ${response.status}`
    // NOTE: For security reasons, some registries respond with 404 on authentication errors as well.
    // So we print authorization info on 404 errors as well.
    if (response.status === 401 || response.status === 403 || response.status === 404) {
      hint = hint ? `${hint}\n\n` : ''
      if (_request.authHeaderValue) {
        hint += `An authorization header was used: ${_request.authHeaderValue}`
      } else {
        hint += 'No authorization header was set for the request.'
      }
    }
    super(`FETCH_${response.status}`, message, { hint })
    this.request = _request
    this.response = response
  }
}

/**
 * Strip `user:pass@` (or `user@`) userinfo that follows a URL scheme in any
 * text, e.g. `GET https://user:pass@host/pkg: …` → `GET https://host/pkg: …`.
 * A registry configured as `https://user:pass@host/` would otherwise leak its
 * embedded basic-auth credentials into every error message that interpolates
 * the request URL (terminal output, CI logs). `FetchError` already hides the
 * auth *header*; this covers credentials carried in the URL itself.
 *
 * Implemented as a single forward scan rather than a regex: `text` is
 * uncontrolled (it interpolates the request URL), so a backtracking pattern is
 * a ReDoS vector, and the scan strips up to the **last** `@` in the authority
 * so a raw `@` inside the password (`user:p@ss@host`) doesn't leak its tail.
 */
export function redactUrlCredentials (text: string): string {
  let result = ''
  let cursor = 0
  while (cursor < text.length) {
    const schemeSep = text.indexOf('://', cursor)
    if (schemeSep === -1) return result + text.slice(cursor)
    const authorityStart = schemeSep + 3
    result += text.slice(cursor, authorityStart)
    cursor = authorityStart
    // Only treat `://` as a URL authority boundary when a scheme character
    // (schemes end in an ASCII alphanumeric) sits right before it; otherwise a
    // bare `://` in the text is left untouched.
    if (schemeSep === 0 || !isSchemeTailChar(text.charCodeAt(schemeSep - 1))) continue
    // Userinfo runs to the last `@` within the authority, which itself ends at
    // the first `/`, `?`, `#`, or whitespace.
    let lastAt = -1
    for (let i = authorityStart; i < text.length; i++) {
      const code = text.charCodeAt(i)
      if (code === 0x2f || code === 0x3f || code === 0x23 || isAsciiWhitespace(code)) break
      if (code === 0x40) lastAt = i
    }
    if (lastAt !== -1) cursor = lastAt + 1
  }
  return result
}

function isSchemeTailChar (code: number): boolean {
  return (code >= 0x30 && code <= 0x39) || (code >= 0x41 && code <= 0x5a) || (code >= 0x61 && code <= 0x7a)
}

function isAsciiWhitespace (code: number): boolean {
  return code === 0x20 || code === 0x09 || code === 0x0a || code === 0x0b || code === 0x0c || code === 0x0d
}

function hideAuthInformation (authHeaderValue: string): string {
  const [authType, token] = authHeaderValue.split(' ')
  if (token == null) return '[hidden]'
  if (token.length < 20) {
    return `${authType} [hidden]`
  }
  return `${authType} ${token.substring(0, 4)}[hidden]`
}

export class LockfileMissingDependencyError extends PnpmError {
  constructor (depPath: string) {
    const message = `Broken lockfile: no entry for '${depPath}' in ${WANTED_LOCKFILE}`
    super('LOCKFILE_MISSING_DEPENDENCY', message, {
      hint: 'This issue is probably caused by a badly resolved merge conflict.\n' +
        'To fix the lockfile, run \'pnpm install --no-frozen-lockfile\'.',
    })
  }
}
