export interface ShorthandContext {
  originalArgv: string[]
  /** The arguments at which the parser stops reading options. */
  escapeArgs?: string[]
  shorthands: Record<string, string | string[]>
}

interface TypedShorthand {
  typed: string
  shorthand: string
}

/**
 * An unknown option that came from a shorthand is confusing when reported by
 * its expanded name: `pnpm update -l` would report `long`, which the user never
 * typed. This finds the shorthand that produced each such option, and replaces
 * the suggestions for it with the shorthand of the other case, if that one
 * expands to an option the command knows (e.g., `-L` for `-l` in `update`).
 *
 * Returns the unknown options that came from shorthands, mapped to the
 * shorthands as typed.
 */
export function attributeUnknownOptionsToShorthands (
  unknownOptions: Map<string, string[]>,
  knownOptions: Set<string>,
  ctx: ShorthandContext
): Map<string, string> {
  const unknownOptionShorthands = new Map<string, string>()
  if (unknownOptions.size === 0) return unknownOptionShorthands
  const { shorthands } = ctx
  for (const { typed, shorthand } of findTypedShorthands(ctx, knownOptions)) {
    for (const optionName of getShorthandOptionNames(shorthands[shorthand])) {
      if (!unknownOptions.has(optionName) || unknownOptionShorthands.has(optionName)) continue
      unknownOptionShorthands.set(optionName, typed)
      unknownOptions.set(optionName, suggestShorthandOfOtherCase(shorthand, shorthands, knownOptions))
    }
  }
  return unknownOptionShorthands
}

/**
 * Like the parser (`@pnpm/nopt`), stops at `--` and at the escape arguments,
 * after which the arguments belong to a script or to the executed command.
 */
function findTypedShorthands (
  { originalArgv, escapeArgs, shorthands }: ShorthandContext,
  knownOptions: Set<string>
): TypedShorthand[] {
  const escapeArgSet = new Set(escapeArgs)
  const endOfOptions = originalArgv.findIndex((arg) => /^-{2,}$/.test(arg) || escapeArgSet.has(arg))
  const args = endOfOptions === -1 ? originalArgv : originalArgv.slice(0, endOfOptions)
  return args.flatMap((arg) => resolveTypedShorthands(arg, shorthands, knownOptions))
}

/**
 * Resolves an argument to shorthands in the same order as `@pnpm/nopt`: an
 * exact option name, an exact shorthand, a group of single-character
 * shorthands (like `-rl`), an option abbreviation, a shorthand abbreviation.
 * An attached value (`--l=false`) is not part of the name.
 */
function resolveTypedShorthands (
  arg: string,
  shorthands: Record<string, string | string[]>,
  knownOptions: Set<string>
): TypedShorthand[] {
  if (arg.length < 2 || !arg.startsWith('-')) return []
  const typed = arg.split('=', 1)[0]
  const name = typed.replace(/^-+/, '')
  if (name === '' || knownOptions.has(name)) return []
  if (Object.hasOwn(shorthands, name)) return [{ typed, shorthand: name }]
  const chars = name.split('')
  if (chars.every((char) => Object.hasOwn(shorthands, char))) {
    return chars.map((char) => ({ typed: `-${char}`, shorthand: char }))
  }
  if (resolveAbbreviation(name, knownOptions) != null) return []
  const shorthand = resolveAbbreviation(name, Object.keys(shorthands))
  return shorthand == null ? [] : [{ typed, shorthand }]
}

function resolveAbbreviation (prefix: string, names: Iterable<string>): string | undefined {
  let match: string | undefined
  for (const name of names) {
    if (!name.startsWith(prefix)) continue
    if (match != null) return undefined
    match = name
  }
  return match
}

function getShorthandOptionNames (expansion: string | string[] | undefined): string[] {
  if (expansion == null) return []
  const args = Array.isArray(expansion) ? expansion : expansion.split(/\s+/)
  const optionNames: string[] = []
  for (const arg of args) {
    const match = /^--(?:no-)?([^=]+)/.exec(arg)
    if (match != null) optionNames.push(match[1])
  }
  return optionNames
}

function suggestShorthandOfOtherCase (
  shorthand: string,
  shorthands: Record<string, string | string[]>,
  knownOptions: Set<string>
): string[] {
  const otherCase = shorthand === shorthand.toLowerCase() ? shorthand.toUpperCase() : shorthand.toLowerCase()
  if (otherCase === shorthand || !Object.hasOwn(shorthands, otherCase)) return []
  const optionNames = getShorthandOptionNames(shorthands[otherCase])
  if (optionNames.length === 0 || !optionNames.every((optionName) => knownOptions.has(optionName))) return []
  return [`-${otherCase}`]
}
