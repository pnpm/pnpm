export default function getPref (
  alias: string,
  name: string,
  version: string,
  opts: {
    pinnedVersion?: 'major' | 'minor' | 'patch',
  },
) {
  const prefix = alias !== name ? `npm:${name}@` : ''
  switch (opts.pinnedVersion || 'major') {
    case 'major':
      return `${prefix}^${version}`
    case 'minor':
      return `${prefix}~${version}`
    case 'patch':
      return `${prefix}${version}`
  }
}
