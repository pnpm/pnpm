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
  relays.set(child, relaySignals(child, { ownProcessGroup }))
  return child
}) as typeof safeExeca

/**
 * Wait for a subprocess started with `trackedExeca`, and report the signal
 * that reached pnpm while it ran, if any. Once the subprocess has ended, the
 * signal relay stops, and after a relayed signal pnpm waits for the
 * subprocess's process group too, so a command that was told to stop
 * finishes shutting down before pnpm goes on.
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
