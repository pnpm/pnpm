export interface SelectedShell {
  sh: string
  shFlag: string
  windowsVerbatimArguments: boolean
}

/**
 * The text `shell` executes for `command`.
 *
 * A Bourne shell that stays the script's parent holds a terminal SIGINT
 * until the foreground command exits, then dies from that signal even when
 * the command already exited with a status. pnpm would report that as a
 * lifecycle failure and lose the command's status
 * (https://github.com/pnpm/pnpm/issues/9945). The trap returns the command's
 * status, and re-raises SIGINT only when the command itself died from it
 * (status 130).
 */
export function scriptBody (shell: SelectedShell, command: string): string {
  if (!returnsInterruptedChildStatus(shell)) return command
  return `${INTERRUPT_STATUS_TRAP}${command}`
}

const INTERRUPT_STATUS_TRAP = "trap 'st=$?; if [ \"$st\" -eq 130 ]; then trap - INT; kill -s INT $$; else exit \"$st\"; fi' INT; "

const BOURNE_SHELLS = new Set(['sh', 'dash', 'bash', 'ash', 'zsh', 'ksh', 'mksh'])

function returnsInterruptedChildStatus (shell: SelectedShell): boolean {
  if (shell.windowsVerbatimArguments || shell.shFlag !== '-c') return false
  const base = shell.sh.split(/[\\/]/).pop()!.toLowerCase().replace(/\.exe$/, '')
  return BOURNE_SHELLS.has(base)
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
