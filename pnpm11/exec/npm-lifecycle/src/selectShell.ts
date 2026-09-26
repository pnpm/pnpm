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

/** `shellEmulator` applies only when `scriptShell` is unset. */
export function useShellEmulator (shellEmulator: boolean | undefined, scriptShell: string | undefined): boolean {
  return shellEmulator === true && !scriptShell
}

/**
 * Whether `cmd.exe` will parse the script, which is what chooses JSON
 * quoting of extra arguments. A configured non-cmd `scriptShell` is
 * quoted for that shell, including when `shellEmulator` is also set.
 */
export function commandParsedByCmd (scriptShell: string | undefined, platform: NodeJS.Platform, shellEmulator: boolean | undefined): boolean {
  if (useShellEmulator(shellEmulator, scriptShell)) return false
  if (platform !== 'win32') return false
  return scriptShell == null || isCmdExe(scriptShell)
}

function isCmdExe (shellPath: string): boolean {
  const basename = shellPath.split(/[\\/]/).pop()!.toLowerCase()
  return basename === 'cmd' || basename === 'cmd.exe'
}
