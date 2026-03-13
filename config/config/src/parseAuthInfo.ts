import { PnpmError } from '@pnpm/error'

/** Authentication information of each registry in the rc file. */
export interface AuthInfo {
  /** Parsed value of `_auth` of each registry in the rc file. */
  authUserPass?: AuthUserPass
  /** The value of `_authToken` of each registry in the rc file. */
  authToken?: string
  /** Parsed value of `tokenHelper` of each registry in the rc file. */
  tokenHelper?: TokenHelper
}

/** Unparsed authentication information of each registry in the rc file. */
export interface AuthInfoInput {
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

export function parseAuthInfo (input: AuthInfoInput): AuthInfo | undefined {
  let authInfo: AuthInfo | undefined

  if (input.tokenHelper) {
    authInfo = {
      ...authInfo,
      tokenHelper: parseTokenHelper(input.tokenHelper),
    }
  }

  if (input.authToken) {
    authInfo = {
      ...authInfo,
      authToken: input.authToken,
    }
  }

  const authUserPass = getAuthUserPass(input)
  if (authUserPass) {
    authInfo = {
      ...authInfo,
      authUserPass,
    }
  }

  return authInfo
}

/** Parsed value of `_auth` of each registry in the rc file. */
export interface AuthUserPass {
  username: string
  password: string
}

/**
 * Extract a pair of username and password from either a base64 encoded string
 * of `<username>:<password>` or a pair of username and password.
 *
 * The function input mirrors the rc file which has 3 properties to define username
 * and password which are: `_auth`, `username`, and `_password`.
 */
function getAuthUserPass ({
  authPairBase64,
  authUsername,
  authPassword,
}: Pick<AuthInfoInput, 'authPairBase64' | 'authUsername' | 'authPassword'>): AuthUserPass | undefined {
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

/** Parsed value of `tokenHelper` of each registry in the rc file. */
export type TokenHelper = [string, ...string[]]

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
