import semver from 'semver'
import type { DependencyPath } from '../graph/types'

export function parseDependencyPath (
  dependencyPath: string,
  ctx: {
    parseVersion: (version: string) => {
      version: string
      peersSuffix?: string
    }
    splitToParts: (dependencyPath: string) => string[]
  }
): DependencyPath {
  if (typeof dependencyPath !== 'string') {
    throw new TypeError(
      `Expected \`dependencyPath\` to be of type \`string\`, got \`${dependencyPath === null ? 'null' : typeof dependencyPath
      }\``
    )
  }
  const isAbsolute = dependencyPath[0] !== '/'
  const parts = ctx.splitToParts(dependencyPath)
  if (!isAbsolute) {
    parts.shift()
  }
  const host = isAbsolute ? parts.shift() : undefined
  if (parts.length === 0) {
    return {
      host,
      isAbsolute,
    }
  }
  const name = parts[0].startsWith('@')
    ? `${parts.shift()}/${parts.shift()}` // eslint-disable-line @typescript-eslint/restrict-template-expressions
    : parts.shift()
  const _version = parts.join('/')
  if (_version) {
    const { version, peersSuffix } = ctx.parseVersion(_version)
    if (semver.valid(version)) {
      return {
        host,
        isAbsolute,
        name,
        peersSuffix,
        version,
      }
    }
  }
  if (!isAbsolute) {
    throw new Error(`${dependencyPath} is an invalid relative dependency path`)
  }
  return {
    host,
    isAbsolute,
  }
}
