export interface ShorthandContext {
  originalArgv: string[]
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
  { originalArgv, shorthands }: ShorthandContext
): Map<string, string> {
  const unknownOptionShorthands = new Map<string, string>()
  if (unknownOptions.size === 0) return unknownOptionShorthands
  for (const { typed, shorthand } of findTypedShorthands(originalArgv, shorthands)) {
    for (const optionName of getShorthandOptionNames(shorthands[shorthand])) {
      if (!unknownOptions.has(optionName) || unknownOptionShorthands.has(optionName)) continue
      unknownOptionShorthands.set(optionName, typed)
      unknownOptions.set(optionName, suggestShorthandOfOtherCase(shorthand, shorthands, knownOptions))
    }
  }
  return unknownOptionShorthands
}

function findTypedShorthands (
  originalArgv: string[],
  shorthands: Record<string, string | string[]>
): TypedShorthand[] {
  const endOfOptions = originalArgv.indexOf('--')
  const args = endOfOptions === -1 ? originalArgv : originalArgv.slice(0, endOfOptions)
  return args.flatMap((arg) => parseTypedShorthands(arg, shorthands))
}

function parseTypedShorthands (
  arg: string,
  shorthands: Record<string, string | string[]>
): TypedShorthand[] {
  const match = /^(--?)([^-=][^=]*)$/.exec(arg)
  if (match == null) return []
  const [, dashes, name] = match
  if (Object.hasOwn(shorthands, name)) {
    return [{ typed: arg, shorthand: name }]
  }
  if (dashes !== '-' || name.length === 1) return []
  // nopt splits a group of single-character shorthands, like `-rl`
  const chars = name.split('')
  if (!chars.every((char) => Object.hasOwn(shorthands, char))) return []
  return chars.map((char) => ({ typed: `-${char}`, shorthand: char }))
}

function getShorthandOptionNames (expansion: string | string[] | undefined): string[] {
  if (expansion == null) return []
  const args = Array.isArray(expansion) ? expansion : [expansion]
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
