const ENV_EXPR = /(?<!\\)(\\*)\$\{([^${}]+)\}/g
const ENV_VALUE = /([^:-]+)(:?)-(.+)/

/**
 * Replace every `${VAR}` (or `${VAR-default}` / `${VAR:-default}`) placeholder
 * in `settingValue` with the env value resolved from `env`. Throws on the first
 * placeholder that has no value and no default.
 */
export function envReplace (settingValue: string, env: NodeJS.ProcessEnv): string {
  return replaceWith(settingValue, env, (orig) => {
    throw new Error(`Failed to replace env in config: ${orig}`)
  })
}

export interface EnvReplaceLossyResult {
  /** The substituted text. Unresolved placeholders are replaced with `''`. */
  value: string
  /**
   * The list of unresolved placeholders (each including its surrounding `${...}`,
   * in source order). One entry per occurrence — duplicates appear multiple times.
   * Callers typically surface each as a warning.
   */
  unresolved: string[]
}

/**
 * Like {@link envReplace}, but replaces unresolved `${VAR}` placeholders with
 * `''` and reports them in {@link EnvReplaceLossyResult.unresolved} instead of
 * throwing. Resolvable placeholders and `${VAR-default}` / `${VAR:-default}`
 * fallbacks still expand normally — only the genuinely unresolved bare ones are
 * dropped.
 *
 * Use this when leaving the literal `${VAR}` in the substituted value would be
 * worse than dropping it (e.g. auth tokens in `.npmrc` under OIDC trusted
 * publishing — see https://github.com/pnpm/pnpm/issues/11513).
 */
export function envReplaceLossy (settingValue: string, env: NodeJS.ProcessEnv): EnvReplaceLossyResult {
  const unresolved: string[] = []
  const value = replaceWith(settingValue, env, (orig) => {
    unresolved.push(orig)
    return ''
  })
  return { value, unresolved }
}

// Shared substitution loop. `onUnresolved` decides what to splice in (and may
// throw) when a placeholder has no value and no default.
function replaceWith (
  settingValue: string,
  env: NodeJS.ProcessEnv,
  onUnresolved: (orig: string) => string
): string {
  return settingValue.replace(ENV_EXPR, (orig: string, escape: string, name: string) => {
    if (escape.length % 2) return orig.slice((escape.length + 1) / 2)
    const halfEscape = escape.slice(escape.length / 2)
    const envValue = getEnvValue(env, name)
    if (envValue === undefined) return `${halfEscape}${onUnresolved(orig)}`
    return `${halfEscape}${envValue}`
  })
}

function getEnvValue (env: NodeJS.ProcessEnv, name: string): string | undefined {
  const matched = name.match(ENV_VALUE)
  if (!matched) return env[name]
  const [, variableName, colon, fallback] = matched
  // Treat `{ KEY: undefined }` as unset rather than "explicitly empty": the
  // `NodeJS.ProcessEnv` (= `Record<string, string | undefined>`) signature lets
  // callers represent an unset variable as a present-but-undefined property,
  // and `${KEY-default}` must reach the fallback in that case. Using
  // `hasOwnProperty` would treat the property as set and return `undefined`
  // instead of `fallback`.
  const v = env[variableName]
  if (v === undefined) return fallback
  return !v && colon ? fallback : v
}
