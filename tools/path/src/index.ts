import path from 'node:path'

export function getToolDirPath (
  opts: {
    pnpmHomeDir: string
    tool: {
      name: string
      version: string
    }
  }
): string {
  return path.join(opts.pnpmHomeDir, '.tools', opts.tool.name.replaceAll('/', '+'), opts.tool.version)
}
