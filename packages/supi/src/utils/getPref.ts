import PnpmError from '@pnpm/error'
import versionSelectorType = require('version-selector-type')

export type PinnedVersion = 'major' | 'minor' | 'patch'

const getPrefix = (alias: string, name: string) => alias !== name ? `npm:${name}@` : ''

export default function getPref (
  alias: string,
  name: string,
  version: string,
  opts: {
    pinnedVersion?: PinnedVersion,
  },
) {
  const prefix = getPrefix(alias, name)
  return `${prefix}${createVersionSpec(version, opts.pinnedVersion)}`
}

export function getPrefPreferSpecifiedSpec (
  opts: {
    alias: string,
    name: string
    version: string,
    rawSpec: string,
    pinnedVersion?: PinnedVersion,
  },
 ) {
  const prefix = getPrefix(opts.alias, opts.name)
  if (opts.rawSpec?.startsWith(`${opts.alias}@${prefix}`)) {
    const selector = versionSelectorType(opts.rawSpec.substr(`${opts.alias}@${prefix}`.length))
    if (selector && (selector.type === 'version' || selector.type === 'range')) {
      return opts.rawSpec.substr(opts.alias.length + 1)
    }
  }
  return `${prefix}${createVersionSpec(opts.version, opts.pinnedVersion)}`
}

export function getPrefPreferSpecifiedExoticSpec (
  opts: {
    alias: string,
    name: string
    version: string,
    rawSpec: string,
    pinnedVersion?: PinnedVersion,
  },
) {
  const prefix = getPrefix(opts.alias, opts.name)
  if (opts.rawSpec?.startsWith(`${opts.alias}@${prefix}`)) {
    const specWithoutName = opts.rawSpec.substr(`${opts.alias}@${prefix}`.length)
    const selector = versionSelectorType(specWithoutName)
    if (!(selector && (selector.type === 'version' || selector.type === 'range'))) {
      return opts.rawSpec.substr(opts.alias.length + 1)
    }
    if (!opts.pinnedVersion) {
      switch (selector.type) {
        case 'version':
          opts.pinnedVersion = 'patch'
          break
        default:
          opts.pinnedVersion = (specWithoutName.startsWith('~') ? 'minor' : (specWithoutName.startsWith('^') ? 'major' : 'patch'))
          break
      }
    }
  }
  return `${prefix}${createVersionSpec(opts.version, opts.pinnedVersion)}`
}

function createVersionSpec (version: string, pinnedVersion?: PinnedVersion) {
  switch (pinnedVersion || 'major') {
    case 'major':
      return `^${version}`
    case 'minor':
      return `~${version}`
    case 'patch':
      return `${version}`
    default:
      throw new PnpmError('BAD_PINNED_VERSION', `Cannot pin '${pinnedVersion}'`)
  }
}
