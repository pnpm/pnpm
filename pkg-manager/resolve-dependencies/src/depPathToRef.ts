import { type Resolution } from '@pnpm/resolver-base'

export function depPathToRef (
  depPath: string,
  opts: {
    alias: string
    realName: string
    resolution: Resolution
  }
) {
  if (opts.resolution.type) return depPath

  if (depPath[0] === '/' && opts.alias === opts.realName) {
    const ref = depPath.replace(`/${opts.realName}@`, '')
    return ref
  }
  return depPath
}
