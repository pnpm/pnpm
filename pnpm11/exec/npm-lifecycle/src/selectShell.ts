export interface SelectedShell {
  sh: string
  shFlag: string
  windowsVerbatimArguments: boolean
}

export function selectShell (scriptShell: string | undefined, platform: NodeJS.Platform, comspec: string | undefined): SelectedShell {
  if (platform === 'win32') {
    const sh = scriptShell ?? comspec ?? 'cmd'
    if (scriptShell == null || isCmdExe(scriptShell)) {
      return { sh, shFlag: '/d /s /c', windowsVerbatimArguments: true }
    }
    return { sh, shFlag: '-c', windowsVerbatimArguments: false }
  }
  return { sh: scriptShell ?? 'sh', shFlag: '-c', windowsVerbatimArguments: false }
}

function isCmdExe (shellPath: string): boolean {
  const basename = shellPath.split(/[\\/]/).pop()!.toLowerCase()
  return basename === 'cmd' || basename === 'cmd.exe'
}
