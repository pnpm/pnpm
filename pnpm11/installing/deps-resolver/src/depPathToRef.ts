export function depPathToRef (
  depPath: string,
  opts: {
    alias: string
    realName: string
  }
): string {
  if (opts.alias === opts.realName && depPath.startsWith(`${opts.realName}@`)) {
    return depPath.substring(opts.realName.length + 1)
  }
  return depPath
}
