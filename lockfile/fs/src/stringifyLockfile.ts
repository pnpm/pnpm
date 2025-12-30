import { type LockfileFile } from '@pnpm/lockfile.types'
import yaml from 'yaml'
import { sortLockfileKeys } from './sortLockfileKeys.js'

const LOCKFILE_YAML_FORMAT: yaml.ToStringOptions = {
  // quotes and shouldn't create them
  lineWidth: 0,

  // The library wraps strings with double quotes by default.
  singleQuote: true,

  // Avoid additional padding between [] and {} when a collection is set to
  // "flow".
  //
  // For example, when flowCollectionPadding is false:
  //
  //   '@esbuild/aix-ppc64@0.25.9':
  //     resolution: { ... }
  //     engines: { node: '>=18' }
  //     cpu: [ ppc64 ]
  //     os: [ aix ]
  //
  // We want flowCollectionPadding to be true, which prints:
  //
  //   '@esbuild/aix-ppc64@0.25.9':
  //     resolution: {...}
  //     engines: {node: '>=18'}
  //     cpu: [ppc64]
  //     os: [aix]
  //
  flowCollectionPadding: false,

  // The pnpm lockfile doesn't use block quotes.
  blockQuote: false,

  // Prevent strings from splitting onto multiple lines. The default is 40.
  doubleQuotedMinMultiLineLength: Number.MAX_SAFE_INTEGER,

  // The lockfile format expects these to be set. The yaml library currently (as
  // of version 2.8.1) defaults to these values, but let's set these explicitly
  // to prevent accidental regressions if the defaults change upstream.
  collectionStyle: 'any',
  directives: false,
  indent: 2,
}

/**
 * Top-level sections of the lockfile that should have each of their entries
 * separated with an extra newline.
 *
 * This significantly reduces the probability of git merge conflicts. It
 * prevents 2 adjacent entries from merge conflicting when they're changed in
 * different branches that later merge together.
 */
const LOOSE_SECTIONS = new Set(['importers', 'packages', 'snapshots'])

// These keys should always render on one line.
const SINGLE_LINE_KEYS: Record<string, true | SingleLineKeyPredicate> = {
  cpu: true,
  engines: true,
  libc: true,
  os: true,
  resolution: isSingleLineResolutionBlock,
}

const RESOLUTIONS_TYPE_EXCEPTIONS = new Set(['variations', 'binary'])

type SingleLineKeyPredicate = (_value: yaml.YAMLMap | yaml.YAMLSeq) => boolean

export function stringifyLockfile (lockfile: LockfileFile): string {
  const sortedLockfile = sortLockfileKeys(lockfile)
  const document = new yaml.Document(sortedLockfile)

  if (!yaml.isMap(document.contents)) {
    throw new Error('Unexpected lockfile YAML document shape. Expected lockfile to be a map.')
  }

  loosenTopLevelKeys(document.contents)
  loosenLargeSections(document.contents)

  yaml.visit(document.contents, {
    Pair (_key, pair, _path) {
      condenseSingleLineKey(pair)
    },
    Scalar (_key, scalar, _path) {
      escapeMultiLineStrings(scalar)
    },
  })

  return document.toString(LOCKFILE_YAML_FORMAT)
}

/**
 * The resolution field should only be on a single line if its type is not in
 * RESOLUTIONS_TYPE_EXCEPTIONS.
 */
function isSingleLineResolutionBlock (value: yaml.YAMLMap<unknown, unknown> | yaml.YAMLSeq<unknown>): boolean {
  const typeField = value.get('type')
  return typeField == null ||
    !yaml.isScalar(typeField) ||
    typeField.value !== 'string' ||
    !RESOLUTIONS_TYPE_EXCEPTIONS.has(typeField.value)
}

function loosenTopLevelKeys (contents: yaml.YAMLMap) {
  for (const pair of contents.items.slice(1)) {
    if (!yaml.isScalar(pair.key)) {
      throw new Error('Encountered unexpected non-scalar key when serializing lockfile.')
    }

    pair.key.spaceBefore = true
  }
}

function loosenLargeSections (contents: yaml.YAMLMap) {
  for (const pair of contents.items) {
    if (!yaml.isScalar(pair.key)) {
      throw new Error('Encountered unexpected non-scalar key when serializing lockfile.')
    }

    const sectionName = pair.key.value
    if (typeof sectionName !== 'string' || !LOOSE_SECTIONS.has(sectionName)) {
      continue
    }

    const section = pair.value
    if (!yaml.isMap(section)) {
      throw new Error(`Unexpected lockfile YAML document shape. Expected '${sectionName}' to be a map.`)
    }

    section.spaceBefore = true

    for (const pair of section.items.slice(1)) {
      if (!yaml.isScalar(pair.key)) {
        throw new Error(`Encountered unexpected non-scalar key in ${sectionName}.`)
      }

      pair.key.spaceBefore = true
    }
  }
}

function condenseSingleLineKey (pair: yaml.Pair) {
  if (!yaml.isScalar(pair.key) || typeof pair.key.value !== 'string' || !yaml.isCollection(pair.value)) {
    return
  }

  const predicate = SINGLE_LINE_KEYS[pair.key.value]

  if (predicate === true || typeof predicate === 'function' && predicate(pair.value)) {
    pair.value.flow = true
  }
}

function escapeMultiLineStrings (scalar: yaml.Scalar<unknown>) {
  if (typeof scalar.value === 'string' && scalar.value.includes('\n')) {
    scalar.type = yaml.Scalar.QUOTE_DOUBLE
  }
}
