export interface SelectedShell {
  sh: string
  shFlag: string
  windowsVerbatimArguments: boolean
}

/**
 * The text `shell` executes for `command`.
 *
 * A Bourne shell that stays the script's parent holds a terminal SIGINT
 * until the foreground command exits. dash and zsh then die from that signal
 * even when the command handled it and exited with a status, so pnpm
 * reported a lifecycle failure and lost the command's status
 * (https://github.com/pnpm/pnpm/issues/9945). The trap makes every such
 * shell do what bash does: re-raise SIGINT when the command died from it
 * (status 130), and otherwise carry on with the rest of the script. A second
 * interrupt always re-raises, so Ctrl+C can still stop a loop of builtins.
 */
export function scriptBody (shell: SelectedShell, command: string): string {
  if (!returnsInterruptedChildStatus(shell)) return command
  return `${INTERRUPT_STATUS_TRAP}${command}`
}

// `$?` must be read first: it is the interrupted command's status only until
// the trap runs a command of its own.
const INTERRUPT_STATUS_TRAP = "trap 'if [ \"$?\" -eq 130 ] || [ -n \"${pnpm_sigint-}\" ]; then trap - INT; kill -s INT $$; fi; pnpm_sigint=1' INT; "

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
