import path from 'path'
import isSubdir from 'is-subdir'

export default function findBestGlobalPrefixOnWindows (
  defaultNpmGlobalPrefix: string,
  env: { [key: string]: string | undefined }
) {
  if (
    (env.LOCALAPPDATA != null && isSubdir(env.LOCALAPPDATA, defaultNpmGlobalPrefix)) ||
    (env.APPDATA != null && isSubdir(env.APPDATA, defaultNpmGlobalPrefix))
  ) {
    return defaultNpmGlobalPrefix
  }
  if (env.APPDATA) return path.join(env.APPDATA, 'npm')
  return defaultNpmGlobalPrefix
}
