import fs from 'node:fs'
import path from 'node:path'

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
 * Resolves once no live process of the group led by `leader` is left, so a
 * script that outlived the shell that started it finishes shutting down.
 * Rejects when the group cannot be observed, so a caller goes on rather
 * than waiting on a question it cannot answer.
 *
 * The group is probed the way the kernel counts it, with a signal of 0. A
 * member that has exited but is not reaped yet still counts there, which
 * happens when pnpm is a container's PID 1 and inherits the orphans, so on
 * Linux such zombies are told apart through `/proc`.
 */
export async function waitForProcessGroup (leader: number, opts?: WaitForProcessGroupOptions): Promise<void> {
  const poll = async (): Promise<void> => {
    if (!hasLiveMembers(leader, opts?.processTable ?? '/proc')) return
    await new Promise<void>((resolve) => setTimeout(resolve, 50))
    return poll()
  }
  return poll()
}

export interface WaitForProcessGroupOptions {
  /** The process table to tell zombies apart in, `/proc` unless a test supplies its own. */
  processTable?: string
}

function hasLiveMembers (group: number, processTable: string): boolean {
  try {
    process.kill(-group, 0)
  } catch (err: unknown) {
    if (errorCode(err) === 'ESRCH') return false
    throw err
  }
  return process.platform !== 'linux' || hasLiveMembersInProc(group, processTable)
}

/**
 * Whether `/proc` lists a process of `group` that is not a zombie. An entry
 * whose state cannot be read is not counted: it has exited since the
 * listing, or it belongs to another user under a restricted process table,
 * and either way it is not a member pnpm started and can observe.
 */
function hasLiveMembersInProc (group: number, processTable: string): boolean {
  return fs.readdirSync(processTable).some((entry) => {
    if (!/^\d+$/.test(entry)) return false
    let stat: string
    try {
      stat = fs.readFileSync(path.join(processTable, entry, 'stat'), 'utf8')
    } catch {
      return false
    }
    // The fields after the parenthesized command name: state, parent, group, ...
    const fields = stat.slice(stat.lastIndexOf(')') + 2).split(' ')
    return fields[0] !== 'Z' && Number(fields[2]) === group
  })
}

function errorCode (err: unknown): unknown {
  return typeof err === 'object' && err != null && 'code' in err ? err.code : undefined
}
