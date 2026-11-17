import nopt from '@pnpm/nopt'

export interface ShorthandContext {
  /** The arguments as passed to the parser. */
  originalArgv: string[]
  types: Parameters<typeof nopt>[0]
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
  for (const { typed, shorthand } of findTypedShorthands(ctx, knownOptions, unknownOptions)) {
    for (const optionName of getShorthandOptionNames(shorthands[shorthand])) {
      if (!unknownOptions.has(optionName) || unknownOptionShorthands.has(optionName)) continue
      unknownOptionShorthands.set(optionName, typed)
      unknownOptions.set(optionName, suggestShorthandOfOtherCase(shorthand, shorthands, knownOptions))
    }
  }
  return unknownOptionShorthands
}

/**
 * An argument that looks like a shorthand is not always read as one: it may be
 * the value of the preceding option, or come after `--` or an escape argument.
 * So only the shorthands that the parser expanded are kept.
 */
function findTypedShorthands (
  ctx: ShorthandContext,
  knownOptions: Set<string>,
  unknownOptions: Map<string, string[]>
): TypedShorthand[] {
  const typedShorthands = ctx.originalArgv.flatMap((arg) => resolveTypedShorthands(arg, ctx.shorthands, knownOptions))
  if (typedShorthands.length === 0) return []
  const expandedShorthands = findExpandedShorthands(ctx, unknownOptions)
  return typedShorthands.filter(({ shorthand }) => expandedShorthands.has(shorthand))
}

const EXPANSION_MARKER = '\0expanded-shorthand:'

/**
 * Parses the arguments again, with each unknown option in a shorthand
 * expansion renamed after the shorthand, and returns the shorthands whose
 * renamed options were parsed. A renamed option keeps the parser type of the
 * original, so the second parse reads the same arguments as options.
 */
function findExpandedShorthands (
  { originalArgv, types, escapeArgs, shorthands }: ShorthandContext,
  unknownOptions: Map<string, string[]>
): Set<string> {
  const shorthandNames = Object.keys(shorthands)
  const markedTypes: Record<string, unknown> = { ...types }
  const markedShorthands: Record<string, string[]> = {}
  for (const [index, shorthand] of shorthandNames.entries()) {
    markedShorthands[shorthand] = renameOptions(shorthands[shorthand], (optionName) => {
      if (!unknownOptions.has(optionName)) return undefined
      const marker = `${EXPANSION_MARKER}${index}:${optionName}`
      if (Object.hasOwn(types, optionName)) markedTypes[marker] = types[optionName]
      return marker
    })
  }
  const parsed = nopt(markedTypes as Parameters<typeof nopt>[0], markedShorthands, originalArgv, 0, { escapeArgs })
  const expandedShorthands = new Set<string>()
  for (const key of Object.keys(parsed)) {
    if (key.startsWith(EXPANSION_MARKER)) {
      expandedShorthands.add(shorthandNames[Number.parseInt(key.slice(EXPANSION_MARKER.length), 10)])
    }
  }
  return expandedShorthands
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

function renameOptions (
  expansion: string | string[],
  getNewName: (optionName: string) => string | undefined
): string[] {
  return splitExpansion(expansion).map((arg) =>
    arg.replace(/^(--(?:no-)?)([^=]+)/, (match, prefix: string, optionName: string) => {
      const newName = getNewName(optionName)
      return newName == null ? match : `${prefix}${newName}`
    })
  )
}

function splitExpansion (expansion: string | string[]): string[] {
  return Array.isArray(expansion) ? expansion : expansion.split(/\s+/)
}

function getShorthandOptionNames (expansion: string | string[] | undefined): string[] {
  if (expansion == null) return []
  const optionNames: string[] = []
  for (const arg of splitExpansion(expansion)) {
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
