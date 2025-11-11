import { getCurrentPackageName } from '@pnpm/cli-meta'
import { installToolToTools } from '@pnpm/tools.installer'
import { type SelfUpdateCommandOptions } from './selfUpdate.js'

export type { SelfUpdateCommandOptions }

export interface InstallPnpmToToolsResult {
  binDir: string
  baseDir: string
  alreadyExisted: boolean
}

export async function installPnpmToTools (pnpmVersion: string, opts: SelfUpdateCommandOptions): Promise<InstallPnpmToToolsResult> {
  const currentPkgName = getCurrentPackageName()

  return installToolToTools({
    pnpmHomeDir: opts.pnpmHomeDir,
    tool: {
      name: currentPkgName,
      version: pnpmVersion,
    },
    additionalPnpmAddArgs: ['--allow-build=@pnpm/exe'],
  })
}
