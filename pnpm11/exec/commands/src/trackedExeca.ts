import { trackChildProcess } from '@pnpm/exec.lifecycle'
import { relaySignals, type SignalRelay, spawnsInOwnProcessGroup } from '@pnpm/exec.npm-lifecycle'
import { safeExeca } from 'execa'

type TrackedChild = ReturnType<typeof safeExeca>

const relays = new WeakMap<TrackedChild, SignalRelay>()

/**
 * `safeExeca`, but the spawned subprocess is registered with the child-process
 * tracker so its process tree can be terminated if pnpm exits on an error while
 * the command is still running (see `trackChildProcess`), and pnpm's own
 * signals are relayed to it the way they are to a lifecycle script. Wait for
 * the subprocess with `waitForTracked`.
 */
export const trackedExeca = ((...args: Parameters<typeof safeExeca>): ReturnType<typeof safeExeca> => {
  const [file, commandArgs, options] = args
  const ownProcessGroup = spawnsInOwnProcessGroup()
  const child = safeExeca(file, commandArgs, { ...options, detached: ownProcessGroup })
  trackChildProcess(child)
  // execa's own cleanup terminates an attached subprocess whenever pnpm
  // exits; a detached one is outside it, so the relay takes that over.
  relays.set(child, relaySignals(child, { ownProcessGroup, terminateOnExit: ownProcessGroup }))
  return child
}) as typeof safeExeca

/**
 * Wait for a subprocess started with `trackedExeca`.
 *
 * Resolves with the first signal that reached pnpm while the subprocess
 * ran, or null when none did, and rejects with the subprocess's failure as
 * execa reports it, signal or no signal, so a command's own exit status is
 * never hidden. Before either, the relay stops, and after a relayed signal
 * pnpm waits for the subprocess's process group too, so a command that was
 * told to stop finishes shutting down before pnpm goes on. After a
 * rejection, `signalReaching` still tells whether a signal was involved.
 */
export async function waitForTracked (child: TrackedChild): Promise<NodeJS.Signals | null> {
  const relay = relays.get(child)
  try {
    await child
  } finally {
    await relay?.settle()
  }
  return relay?.interruptedBy() ?? null
}

/** The first signal that reached pnpm while `child` ran, or null. */
export function signalReaching (child: TrackedChild | undefined): NodeJS.Signals | null {
  return child ? relays.get(child)?.interruptedBy() ?? null : null
}
