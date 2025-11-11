import { installToolToTools } from '@pnpm/tools.installer'

export interface InstallNpmToToolsOptions {
  pnpmHomeDir: string
}

export interface InstallNpmToToolsResult {
  alreadyExisted: boolean
  baseDir: string
  binDir: string
}

export async function installNpmToTools (
  npmVersion: string,
  opts: InstallNpmToToolsOptions
): Promise<InstallNpmToToolsResult> {
  return installToolToTools({
    pnpmHomeDir: opts.pnpmHomeDir,
    tool: {
      name: 'npm',
      version: npmVersion,
    },
  })
}
