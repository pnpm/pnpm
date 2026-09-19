import { execFile } from 'node:child_process'
import fs from 'node:fs'

/**
 * Whether this process has a controlling terminal.
 *
 * Ctrl+C there interrupts the whole foreground process group at once, and a
 * child in that group has the SIGINT already. Relaying it would deliver a
 * second one, which ends a child that handled the first and then left the
 * default action in place (https://github.com/pnpm/pnpm/issues/7374).
 * Node.js cannot ask who sent a signal, so the terminal stands in for it:
 * without one, only kill() can reach the process, and the child needs the relay.
 */
export function hasControllingTerminal (): boolean {
  if (process.platform === 'win32') return false
  let tty: number
  try {
    tty = fs.openSync('/dev/tty', fs.constants.O_RDONLY | fs.constants.O_NOCTTY)
  } catch {
    return false
  }
  fs.closeSync(tty)
  return true
}

/**
 * Whether a child should get a process group of its own.
 *
 * Without a controlling terminal, nothing but pnpm signals the child, and
 * the shell running the script may not pass a signal on: a sh that stays
 * the script's parent dies from SIGTERM at once and holds a SIGINT until
 * its child exits. Signalling the whole group reaches the script. With a
 * terminal the child stays in the foreground group, where the terminal's
 * signals reach it and it may read the terminal.
 */
export function spawnsInOwnProcessGroup (): boolean {
  return process.platform !== 'win32' && !hasControllingTerminal()
}

/**
 * Resolves once no live process of the group led by `leader` is left, so a
 * script that outlived the shell that started it finishes shutting down.
 * A member that has exited but is not reaped yet no longer counts.
 */
export async function waitForProcessGroup (leader: number): Promise<void> {
  if (!await hasLiveMembers(leader)) return
  await new Promise<void>((resolve) => setTimeout(resolve, 50))
  return waitForProcessGroup(leader)
}

function hasLiveMembers (group: number): Promise<boolean> {
  return new Promise((resolve) => {
    execFile('ps', ['-A', '-o', 'pgid=', '-o', 'stat='], (err, stdout) => {
      if (err) {
        resolve(false)
        return
      }
      resolve(stdout.split('\n').some((line) => {
        const [groupId, stat] = line.trim().split(/\s+/)
        return Number(groupId) === group && stat != null && !stat.startsWith('Z')
      }))
    })
  })
}
