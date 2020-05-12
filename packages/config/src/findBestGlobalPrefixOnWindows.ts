import isSubdir = require('is-subdir')
import path = require('path')

export default function findBestGlobalPrefixOnWindows (
  defaultNpmGlobalPrefix: string,
  env: { [key: string]: string | undefined }
) {
  if (
    env.LOCALAPPDATA && isSubdir(env.LOCALAPPDATA, defaultNpmGlobalPrefix) ||
    env.APPDATA && isSubdir(env.APPDATA, defaultNpmGlobalPrefix)
  ) {
    return defaultNpmGlobalPrefix
  }
  if (env.APPDATA) return path.join(env.APPDATA, 'npm')
  return defaultNpmGlobalPrefix
}
