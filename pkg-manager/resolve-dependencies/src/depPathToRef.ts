import { type Resolution } from '@pnpm/resolver-base'

export function depPathToRef (
  depPath: string,
  opts: {
    alias: string
    realName: string
    resolution: Resolution
  }
) {
  if (opts.alias === opts.realName && depPath.startsWith(`${opts.realName}@`)) {
    return depPath.substring(opts.realName.length + 1)
  }
  return depPath
}
