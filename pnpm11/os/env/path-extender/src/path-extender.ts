import { PnpmError } from '@pnpm/error'
import {
  addDirToPosixEnvPath,
  type AddDirToPosixEnvPathOpts,
  type ConfigFileChangeType,
  type ConfigReport,
  type PathExtenderPosixReport,
} from '@pnpm/os.env.path-extender-posix'
import { addDirToWindowsEnvPath, type PathExtenderWindowsReport } from '@pnpm/os.env.path-extender-windows'

export type { ConfigFileChangeType, ConfigReport }

export type PathExtenderReport = Pick<PathExtenderPosixReport, 'oldSettings' | 'newSettings'> & Partial<Pick<PathExtenderPosixReport, 'configFile'>>

export type AddDirToEnvPathOpts = AddDirToPosixEnvPathOpts

export async function addDirToEnvPath (dir: string, opts: AddDirToEnvPathOpts): Promise<PathExtenderReport> {
  if (opts.proxyVarSubDir) {
    if (
      opts.proxyVarSubDir.startsWith('/') ||
      opts.proxyVarSubDir.startsWith('\\') ||
      opts.proxyVarSubDir.includes('..') ||
      /[;%"'`$<>&|\n\r]/.test(opts.proxyVarSubDir)
    ) {
      throw new PnpmError('INVALID_SUBDIR', `Invalid proxyVarSubDir: "${opts.proxyVarSubDir}"`)
    }
  }
  if (process.platform === 'win32') {
    return renderWindowsReport(await addDirToWindowsEnvPath(dir, {
      position: opts.position,
      proxyVarName: opts.proxyVarName,
      proxyVarSubDir: opts.proxyVarSubDir,
      overwriteProxyVar: opts.overwrite,
    })
    )
  }
  return addDirToPosixEnvPath(dir, opts)
}

export function renderWindowsReport (changedEnvVariables: PathExtenderWindowsReport): PathExtenderReport {
  const oldSettings: string[] = []
  const newSettings: string[] = []
  for (const changedEnvVariable of changedEnvVariables) {
    if (changedEnvVariable.oldValue) {
      oldSettings.push(`${changedEnvVariable.variable}=${changedEnvVariable.oldValue}`)
    }
    if (changedEnvVariable.newValue) {
      newSettings.push(`${changedEnvVariable.variable}=${changedEnvVariable.newValue}`)
    }
  }
  return {
    oldSettings: oldSettings.join('\n'),
    newSettings: newSettings.join('\n'),
  }
}
