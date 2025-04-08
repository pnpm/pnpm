import { type Dependencies } from '@pnpm/types'

/**
 * Currently, `pnpm update` relies on recreating CLI params to pass to `installDeps`.
 *
 * For `pnpm update` to correctly modify the spec of `jsr:` dependencies, the CLI params must
 * be bare `jsr:` prefs without tags.
 *
 * This function checks if the {@link alias} is a `jsr:` dependency and returns a bare `jsr:` pref
 * if it is. Otherwise, it returns {@link alias} as-is.
*/
export function createJsrParamWithoutTag (dependencies: Dependencies, alias: string): string {
  const pref: string | undefined = dependencies[alias]
  if (!pref?.startsWith('jsr:')) return alias

  const prefWithoutJsr = pref.slice('jsr:'.length)

  // convert: jsr:@<scope>/<name>[@<tag>] → <alias>@jsr:@<scope>/<name>
  if (prefWithoutJsr.startsWith('@')) {
    const index = prefWithoutJsr.lastIndexOf('@')

    // convert: jsr:@<scope>/<name> → <alias>@jsr:@<scope>/<name>
    if (index < 1) {
      return `${alias}@jsr:${prefWithoutJsr}`
    }

    // convert: jsr:@<scope>/<name>@<tag> → <alias>@jsr:@<scope>/<name>
    const prefWithoutTag = prefWithoutJsr.slice(0, index)
    return `${alias}@jsr:${prefWithoutTag}`
  }

  // convert: jsr:<tag> → jsr:<alias>
  return `jsr:${alias}`
}
