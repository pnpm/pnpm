import PnpmError from '@pnpm/error'

export type PinnedVersion = 'major' | 'minor' | 'patch' | 'none'

export const getPrefix = (alias: string, name: string) => alias !== name ? `npm:${name}@` : ''

export function getPref (
  alias: string,
  name: string,
  version: string | undefined,
  opts: {
    pinnedVersion?: PinnedVersion
  }
) {
  const prefix = getPrefix(alias, name)
  return `${prefix}${createVersionSpec(version, { pinnedVersion: opts.pinnedVersion })}`
}

export function createVersionSpec (version: string | undefined, opts: { pinnedVersion?: PinnedVersion, rolling?: boolean }) {
  switch (opts.pinnedVersion ?? 'major') {
  case 'none':
    return '*'
  case 'major':
    if (opts.rolling) return '^'
    return !version ? '*' : `^${version}`
  case 'minor':
    if (opts.rolling) return '~'
    return !version ? '*' : `~${version}`
  case 'patch':
    if (opts.rolling) return '*'
    return !version ? '*' : `${version}`
  default:
    throw new PnpmError('BAD_PINNED_VERSION', `Cannot pin '${opts.pinnedVersion ?? 'undefined'}'`)
  }
}
