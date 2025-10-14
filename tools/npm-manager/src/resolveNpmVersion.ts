import path from 'path'
import { installNpmToTools, type InstallNpmToToolsOptions } from './installNpmToTools.js'

export interface ResolveNpmVersionResult {
  npmPath: string
  npmVersion: string
}

export async function resolveNpmVersion (
  wantedNpmVersion: string,
  opts: InstallNpmToToolsOptions
): Promise<ResolveNpmVersionResult> {
  const { binDir } = await installNpmToTools(wantedNpmVersion, opts)
  const npmPath = path.join(binDir, 'npm')

  return {
    npmPath,
    npmVersion: wantedNpmVersion,
  }
}
