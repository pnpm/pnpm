import { createJsrPref, parseJsrSpec } from '@pnpm/jsr-specs'
import { type Dependencies } from '@pnpm/types'

/**
 * Currently, `pnpm update` relies on recreating CLI params to pass to `installDeps`.
 *
 * For `pnpm update` to correctly modify the spec of `jsr:` dependencies, the CLI params must
 * be bare `jsr:` specification that specifies only scope and name (no version, range, nor tag).
 *
 * This function checks if the {@link alias} is a `jsr:` dependency and returns a bare `jsr:` pref
 * if it is. Otherwise, it returns {@link alias} as-is.
 */
export function createJsrParamWithoutSpec (dependencies: Dependencies, alias: string): string {
  const pref: string | undefined = dependencies[alias]
  if (pref == null) return alias

  const spec = parseJsrSpec(pref)
  if (spec == null) return alias

  // convert: jsr:<spec> → jsr:<alias>
  if (spec.scope == null) {
    return `jsr:${alias}`
  }

  // convert: jsr:@<scope>/<name>[@<spec>] → <alias>@jsr:@<scope>/<name>
  delete spec.spec
  return `${alias}@${createJsrPref(spec)}`
}
