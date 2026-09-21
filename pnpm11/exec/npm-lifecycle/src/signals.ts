import fs from 'node:fs'
import path from 'node:path'

/** A child pnpm relays its signals to. */
export interface SignalTarget {
  pid?: number
  kill: (signal?: NodeJS.Signals | number) => boolean
}

export interface RelaySignalsOptions {
  /** The child leads a process group of its own, which is what pnpm signals. */
  ownProcessGroup: boolean
  /** Raise pnpm's interrupt after every relay in the active group settles. */
  raiseOnInterrupt?: boolean
  /**
   * Terminate a child still running when pnpm exits. A caller whose child
   * is terminated on exit by other means leaves this off, or the child gets
   * a second SIGTERM in the middle of its shutdown.
   */
  terminateOnExit: boolean
}

/** Handles pnpm's own signals on behalf of a running child. */
export interface SignalRelay {
  /** The first signal that reached pnpm while the child ran, if any. */
  interruptedBy: () => NodeJS.Signals | null
  /**
   * Whether pnpm passed a signal on to the child. A SIGINT that a terminal
   * delivered to the child along with pnpm does not count: the child had it
   * already, and pnpm relayed nothing.
   */
  relayed: () => boolean
  /** Terminate the child as pnpm's own exit would, once. */
  terminate: () => void
  /** Raise a signal on pnpm once the children running alongside this one have settled. */
  raise: (signal: NodeJS.Signals) => Promise<void>
  /**
   * Wait for the child's process group after a relayed signal, then stop
   * relaying. The shell may have died from the signal while the script it
   * started is still shutting down, so the wait has no deadline of its own;
   * the relay stays on meanwhile, and further signals escalate as they
   * always do, the last of them ending pnpm itself. An interrupted group
   * settles together with its pending raise, so callers cannot dispatch
   * replacement work while the other children are still shutting down.
   */
  settle: () => Promise<void>
}

/**
 * Relay pnpm's own signals to `child` until `settle` is called.
 *
 * A SIGTERM is passed on. A SIGINT is passed on unless a terminal delivered
 * it, in which case the child has it already; a second SIGINT becomes a
 * SIGTERM. A child with a process group of its own is signalled as a group.
 */
export function relaySignals (child: SignalTarget, opts: RelaySignalsOptions): SignalRelay {
  const group = joinRelayGroup()
  let interruptedBy: NodeJS.Signals | null = null
  let relayed = false
  let settled = false
  let terminated = false
  const prepareRaise = (signal: NodeJS.Signals): void => {
    group.interrupted = true
    if (group.signalToRaise == null || signal === 'SIGTERM') {
      group.signalToRaise = signal
    }
    group.raised ??= group.settled.then(() => {
      process.kill(process.pid, group.signalToRaise!)
      // Signal delivery is asynchronous. Leave it a turn to end pnpm before
      // a caller reports the child as an ordinary command failure.
      return new Promise<void>((resolve) => setTimeout(resolve, 1000))
    })
  }
  const relay = (signal: NodeJS.Signals): void => {
    relayed = true
    if (opts.ownProcessGroup && child.pid != null) {
      try {
        process.kill(-child.pid, signal)
      } catch {
        // the group is gone already
      }
      return
    }
    child.kill(signal)
  }
  const terminate = (): void => {
    if (terminated) return
    terminated = true
    relay('SIGTERM')
  }
  const onTerm = (): void => {
    interruptedBy ??= 'SIGTERM'
    group.interrupted = true
    if (opts.raiseOnInterrupt) prepareRaise('SIGTERM')
    terminate()
  }
  const onEscalate = (): void => {
    if (opts.raiseOnInterrupt) prepareRaise('SIGTERM')
    terminate()
  }
  const onInterrupt = (): void => {
    interruptedBy ??= 'SIGINT'
    group.interrupted = true
    if (opts.raiseOnInterrupt) prepareRaise('SIGINT')
    if (!hasControllingTerminal()) {
      relay('SIGINT')
    }
    process.once('SIGINT', onEscalate)
  }
  process.once('SIGTERM', onTerm)
  process.once('SIGINT', onInterrupt)
  if (opts.terminateOnExit) {
    process.on('exit', terminate)
  }
  return {
    interruptedBy: () => interruptedBy,
    relayed: () => relayed,
    raise: async (signal) => {
      prepareRaise(signal)
      await group.raised
    },
    terminate,
    settle: async () => {
      try {
        if (relayed && opts.ownProcessGroup && child.pid != null) {
          await waitForProcessGroup(child.pid)
        }
      } catch {
        // The group cannot be observed, so there is nothing to wait on.
      } finally {
        process.removeListener('SIGTERM', onTerm)
        process.removeListener('SIGINT', onEscalate)
        process.removeListener('SIGINT', onInterrupt)
        process.removeListener('exit', terminate)
        if (!settled) {
          settled = true
          group.active -= 1
          if (group.active === 0) group.resolve()
        }
      }
      if (group.interrupted || group.raised != null) {
        await group.settled
        await group.raised
      }
    },
  }
}

interface RelayGroup {
  active: number
  interrupted: boolean
  signalToRaise?: NodeJS.Signals
  settled: Promise<void>
  resolve: () => void
  raised?: Promise<void>
}

let currentRelayGroup: RelayGroup | undefined

function joinRelayGroup (): RelayGroup {
  if (currentRelayGroup == null || currentRelayGroup.active === 0 || currentRelayGroup.interrupted) {
    let resolve!: () => void
    const settled = new Promise<void>((resolvePromise) => {
      resolve = resolvePromise
    })
    currentRelayGroup = { active: 0, interrupted: false, settled, resolve }
  }
  currentRelayGroup.active += 1
  return currentRelayGroup
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
