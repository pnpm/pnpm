import PnpmError from '@pnpm/error'

export type PinnedVersion = 'major' | 'minor' | 'patch' | 'none'

export const getPrefix = (alias: string, name: string) => alias !== name ? `npm:${name}@` : ''

export function getPref (
  alias: string,
  name: string,
  version: string,
  opts: {
    pinnedVersion?: PinnedVersion
  }
) {
  const prefix = getPrefix(alias, name)
  return `${prefix}${createVersionSpec(version, opts.pinnedVersion)}`
}

export function createVersionSpec (version: string, pinnedVersion?: PinnedVersion) {
  switch (pinnedVersion ?? 'major') {
  case 'none':
    return '*'
  case 'major':
    return `^${version}`
  case 'minor':
    return `~${version}`
  case 'patch':
    return `${version}`
  default:
    throw new PnpmError('BAD_PINNED_VERSION', `Cannot pin '${pinnedVersion ?? 'undefined'}'`)
  }
}
