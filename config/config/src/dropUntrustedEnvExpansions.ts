import util from 'util'
import { envReplace } from '@pnpm/config.env-replace'
import { readIniFileSync } from 'read-ini-file'

const AUTH_VALUE_KEYS = ['_authToken', '_auth', '_password', 'username', 'tokenHelper', 'cert', 'key'] as const
const AUTH_VALUE_KEY_SUFFIXES = AUTH_VALUE_KEYS.map(key => `:${key}`)

/**
 * Removes from `source` (a parsed npm-conf source layer) every setting whose
 * raw form in the `.npmrc` file at `filePath` uses a `${...}` placeholder in
 * a request destination (registry/proxy URLs, URL-scoped keys) or in a
 * registry credential value. Repository-controlled `.npmrc` files must not
 * be able to expand environment variables into the URLs pnpm sends requests
 * to or into the credentials attached to them.
 */
export function dropUntrustedEnvExpansions (
  source: Record<string, unknown>,
  filePath: string,
  warnings: string[]
): void {
  let raw: Record<string, unknown>
  try {
    raw = readIniFileSync(filePath) as Record<string, unknown>
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && (err.code === 'ENOENT' || err.code === 'EISDIR')) {
      return
    }
    throw err
  }
  for (const [rawKey, rawValue] of Object.entries(raw)) {
    let expandedKey = rawKey
    if (hasEnvPlaceholder(rawKey)) {
      try {
        expandedKey = envReplace(rawKey, process.env)
      } catch {
        // npm-conf failed to expand this key too, so it never reached `source`.
        continue
      }
      if (isRequestDestinationKey(rawKey) || isRequestDestinationKey(expandedKey)) {
        deleteOwnSetting(source, expandedKey)
        warnIgnoredRequestDestinationEnv(filePath, rawKey, warnings)
        continue
      }
      if (isAuthValueKey(rawKey) || isAuthValueKey(expandedKey)) {
        deleteOwnSetting(source, expandedKey)
        warnIgnoredAuthValueEnv(filePath, rawKey, warnings)
        continue
      }
    }
    if (typeof rawValue === 'string' && hasEnvPlaceholder(rawValue)) {
      if (isRequestDestinationValueKey(expandedKey)) {
        deleteOwnSetting(source, expandedKey)
        warnIgnoredRequestDestinationEnv(filePath, expandedKey, warnings)
        continue
      }
      if (isAuthValueKey(expandedKey)) {
        deleteOwnSetting(source, expandedKey)
        warnIgnoredAuthValueEnv(filePath, expandedKey, warnings)
      }
    }
  }
}

function deleteOwnSetting (source: Record<string, unknown>, key: string): void {
  // npm-conf sources are chained via prototype, so only delete own keys.
  if (Object.hasOwn(source, key)) {
    delete source[key]
  }
}

function isRequestDestinationKey (key: string): boolean {
  return isRegistryKey(key) || key.startsWith('//')
}

function isRequestDestinationValueKey (key: string): boolean {
  return isRegistryKey(key) || key === 'https-proxy' || key === 'http-proxy' || key === 'proxy'
}

function isRegistryKey (key: string): boolean {
  return key === 'registry' || (key.startsWith('@') && key.endsWith(':registry'))
}

function isAuthValueKey (key: string): boolean {
  return (AUTH_VALUE_KEYS as readonly string[]).includes(key) || AUTH_VALUE_KEY_SUFFIXES.some(suffix => key.endsWith(suffix))
}

export function hasEnvPlaceholder (value: string): boolean {
  return /\$\{[^}]+\}/.test(value)
}

const DOCS_URL = 'https://pnpm.io/npmrc'

// A runnable `pnpm config set` example is only safe to suggest when the key has
// no `${...}` placeholder — embedding such a key in a shell command would let
// the shell expand it on copy-paste, producing a different key and possibly
// leaking an env value into shell history.
function configSetExample (key: string): string {
  return hasEnvPlaceholder(key) ? '' : ` (for example, run: pnpm config set "${key}" <value>)`
}

function warnIgnoredRequestDestinationEnv (filePath: string, key: string, warnings: string[]): void {
  warnings.push(`Ignored project-level request destination "${key}" in "${filePath}": ` +
    'environment variables are not expanded in registry or proxy URLs that come from a project .npmrc, ' +
    'because that file is committed to the repository and a malicious value could redirect requests or leak secrets. ' +
    'Move this setting to a trusted source that pnpm still expands — put it in your user-level ~/.npmrc, ' +
    `or set it with pnpm config set${configSetExample(key)}. ` +
    `If the value is not secret, you can also write it literally in the project .npmrc. See ${DOCS_URL}`)
}

function warnIgnoredAuthValueEnv (filePath: string, key: string, warnings: string[]): void {
  warnings.push(`Ignored project-level auth setting "${key}" in "${filePath}": ` +
    'environment variables are not expanded in registry credentials that come from a project .npmrc, ' +
    'because that file is committed to the repository and could leak the secret to an attacker-controlled registry. ' +
    'Move this credential to a trusted source that pnpm still expands — put the line in your user-level ~/.npmrc, ' +
    `or set it with pnpm config set${configSetExample(key)}. See ${DOCS_URL}`)
}
