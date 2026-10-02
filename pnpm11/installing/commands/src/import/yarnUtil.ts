/**
 * https://github.com/snyk/nodejs-lockfile-parser/blob/a4557f0015d0299045997b454cea9e91da2501de/lib/parsers/yarn-utils.ts
 */
import type { structUtils } from '@yarnpkg/core'

const BUILTIN_PLACEHOLDER = 'builtin'
const MULTIPLE_KEYS_REGEXP = / *, */

export type ParseDescriptor = typeof structUtils.parseDescriptor
export type ParseRange = typeof structUtils.parseRange

type YarnRange = ReturnType<ParseRange>

const keyNormalizer = (
  parseDescriptor: ParseDescriptor,
  parseRange: ParseRange
) => (rawDescriptor: string): string[] => {
  // See https://yarnpkg.com/features/protocols
  const descriptor = parseDescriptor(rawDescriptor)
  const name = `${descriptor.scope ? '@' + descriptor.scope + '/' : ''}${
    descriptor.name
  }`
  return [rawDescriptor, ...normalizeRange(name, parseRange(descriptor.range))]
}

function normalizeRange (name: string, range: YarnRange): string[] {
  const protocol = range.protocol
  if (protocol == null) {
    return [range.source ? `${name}@${range.source}#${range.selector}` : `${name}@${range.selector}`]
  }
  switch (protocol) {
    case 'npm:':
    case 'file:':
      return [`${name}@${range.selector}`, `${name}@${protocol}${range.selector}`]
    case 'git:':
    case 'git+ssh:':
    case 'git+http:':
    case 'git+https:':
    case 'github:':
      return [range.source ? formatSourceDescriptor(name, range) : `${name}@${protocol}${range.selector}`]
    case 'patch:':
      return [range.source && range.selector.startsWith(BUILTIN_PLACEHOLDER) ? range.source : formatSourceDescriptor(name, range)]
    case 'http:':
    case 'https:':
    case 'link:':
    case 'portal:':
    case 'exec:':
    case 'workspace:':
    case 'virtual:':
    default:
    // For user defined plugins
      return [`${name}@${protocol}${range.selector}`]
  }
}

function formatSourceDescriptor (name: string, range: YarnRange): string {
  return `${name}@${range.protocol}${range.source}${
    range.selector ? '#' + range.selector : ''
  }`
}

export type YarnLockFileKeyNormalizer = (fullDescriptor: string) => Set<string>

export const yarnLockFileKeyNormalizer = (
  parseDescriptor: ParseDescriptor,
  parseRange: ParseRange
): YarnLockFileKeyNormalizer => (fullDescriptor: string) => {
  const allKeys = fullDescriptor
    .split(MULTIPLE_KEYS_REGEXP)
    .map(keyNormalizer(parseDescriptor, parseRange))
  return new Set<string>(allKeys.flat(5))
}
