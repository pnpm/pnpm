import { PnpmError } from '@pnpm/error'
import type { BasicAuth, Creds, TokenHelper } from '@pnpm/types'

export type { BasicAuth, Creds, TokenHelper }

/** Unparsed authentication information of each registry in the rc file. */
export interface RawCreds {
  /** Value of `_authToken` in the rc file. */
  authToken?: string
  /** Value of `_auth` in the rc file. */
  authPairBase64?: string
  /** Value of `username` in the rc file. */
  authUsername?: string
  /** Value of `_password` in the rc file. */
  authPassword?: string
  /** Value of `tokenHelper` in the rc file. */
  tokenHelper?: string
}

export function parseCreds (input: RawCreds): Creds | undefined {
  let parsedCreds: Creds | undefined

  if (input.tokenHelper) {
    parsedCreds = {
      ...parsedCreds,
      tokenHelper: parseTokenHelper(input.tokenHelper),
    }
  }

  if (input.authToken) {
    parsedCreds = {
      ...parsedCreds,
      authToken: input.authToken,
    }
  }

  const basicAuth = parseBasicAuth(input)
  if (basicAuth) {
    parsedCreds = {
      ...parsedCreds,
      basicAuth,
    }
  }

  return parsedCreds
}


/**
 * Extract a pair of username and password from either a base64 encoded string
 * of `<username>:<password>` or a pair of username and password.
 *
 * The function input mirrors the rc file which has 3 properties to define username
 * and password which are: `_auth`, `username`, and `_password`.
 */
function parseBasicAuth ({
  authPairBase64,
  authUsername,
  authPassword,
}: Pick<RawCreds, 'authPairBase64' | 'authUsername' | 'authPassword'>): BasicAuth | undefined {
  if (authPairBase64) {
    const pair = atob(authPairBase64)
    const colonIndex = pair.indexOf(':')
    if (colonIndex < 0) {
      throw new AuthMissingSeparatorError()
    }
    const username = pair.slice(0, colonIndex)
    const password = pair.slice(colonIndex + 1)
    return { username, password }
  }

  if (authUsername && authPassword) {
    return { username: authUsername, password: atob(authPassword) }
  }

  return undefined
}

export class AuthMissingSeparatorError extends PnpmError {
  constructor () {
    super('AUTH_MISSING_SEPARATOR', 'No separator found in the decoded form of _auth', {
      hint: '_auth is a base64 encoded form of <username>:<password> where the colon (:) serves as the separator',
    })
  }
}


/** Characters reserved for more advanced features in the future. */
const RESERVED_CHARACTERS = new Set(['$', '%', '`', '"', "'"])

/**
 * Parse a value of `tokenHelper` from an rc file into an array of
 * token helper command and its arguments.
 */
function parseTokenHelper (source: string): TokenHelper {
  source = source.trim()

  for (const char of source) {
    // We'll only support a simple syntax for now.
    // In the future, we may add quotations and environment variable interpolations.
    if (RESERVED_CHARACTERS.has(char)) {
      throw new TokenHelperUnsupportedCharacterError(char)
    }
  }

  const command = source.split(/\s+/).filter(Boolean)

  return command as [string, ...string[]]
}

export class TokenHelperUnsupportedCharacterError extends PnpmError {
  readonly char: string
  constructor (char: string) {
    let hint = 'Try wrapping the current command in a script whose name does not contain unsupported characters'
    if (char === '"' || char === "'") {
      hint = `pnpm does not support quotations in tokenHelper. ${hint}`
    } else if (char === '$' || char === '%') {
      hint = `pnpm does not support environment variables. ${hint}`
    }
    super('TOKEN_HELPER_UNSUPPORTED_CHARACTER', `Unexpected character ${JSON.stringify(char)}`, { hint })
    this.char = char
  }
}
