/**
 * https://github.com/snyk/nodejs-lockfile-parser/blob/master/lib/parsers/yarn-utils.ts
 */
import { structUtils } from '@yarnpkg/core'

const BUILTIN_PLACEHOLDER = 'builtin'
const MULTIPLE_KEYS_REGEXP = / *, */g

export type ParseDescriptor = typeof structUtils.parseDescriptor
export type ParseRange = typeof structUtils.parseRange

const keyNormalizer = (
  parseDescriptor: ParseDescriptor,
  parseRange: ParseRange
) => (rawDescriptor: string): string[] => {
  // See https://yarnpkg.com/features/protocols
  const descriptors: string[] = [rawDescriptor]
  const descriptor = parseDescriptor(rawDescriptor)
  const name = `${descriptor.scope ? '@' + descriptor.scope + '/' : ''}${
    descriptor.name
  }`
  const range = parseRange(descriptor.range)
  const protocol = range.protocol
  switch (protocol) {
  case 'npm:':
  case 'file:':
    descriptors.push(`${name}@${range.selector}`)
    descriptors.push(`${name}@${protocol}${range.selector}`)
    break
  case 'git:':
  case 'git+ssh:':
  case 'git+http:':
  case 'git+https:':
  case 'github:':
    if (range.source) {
      descriptors.push(
        `${name}@${protocol}${range.source}${
          range.selector ? '#' + range.selector : ''
        }`
      )
    } else {
      descriptors.push(`${name}@${protocol}${range.selector}`)
    }
    break
  case 'patch:':
    if (range.source && range.selector.indexOf(BUILTIN_PLACEHOLDER) === 0) {
      descriptors.push(range.source)
    } else {
      descriptors.push(
        // eslint-disable-next-line
        `${name}@${protocol}${range.source}${
          range.selector ? '#' + range.selector : ''
        }`
      )
    }
    break
  case null:
  case undefined:
    if (range.source) {
      descriptors.push(`${name}@${range.source}#${range.selector}`)
    } else {
      descriptors.push(`${name}@${range.selector}`)
    }
    break
  case 'http:':
  case 'https:':
  case 'link:':
  case 'portal:':
  case 'exec:':
  case 'workspace:':
  case 'virtual:':
  default:
    // For user defined plugins
    descriptors.push(`${name}@${protocol}${range.selector}`)
    break
  }
  return descriptors
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
