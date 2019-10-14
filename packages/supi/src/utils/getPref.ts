import PnpmError from '@pnpm/error'
import versionSelectorType = require('version-selector-type')

export default function getPref (
  alias: string,
  name: string,
  version: string,
  opts: {
    rawSpec?: string,
    pinnedVersion?: 'major' | 'minor' | 'patch',
  },
) {
  const prefix = alias !== name ? `npm:${name}@` : ''
  if (opts.rawSpec?.startsWith(`${alias}@${prefix}`)) {
    const selector = versionSelectorType(opts.rawSpec.substr(`${alias}@${prefix}`.length))
    if (selector && (selector.type === 'version' || selector.type === 'range')) {
      return opts.rawSpec.substr(alias.length + 1)
    }
  }
  switch (opts.pinnedVersion || 'major') {
    case 'major':
      return `${prefix}^${version}`
    case 'minor':
      return `${prefix}~${version}`
    case 'patch':
      return `${prefix}${version}`
    default:
      throw new PnpmError('BAD_PINNED_VERSION', `Cannot pin '${opts.pinnedVersion}'`)
  }
}
