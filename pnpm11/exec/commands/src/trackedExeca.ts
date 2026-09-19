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
 * Wait for a subprocess started with `trackedExeca`.
 *
 * Resolves with the first signal that reached pnpm while the subprocess
 * ran, or null when none did. A subprocess that failed after such a signal
 * did so because of it, so the failure is not reported; without a signal
 * the failure is rethrown as execa reports it. Before either, the relay
 * stops, and after a relayed signal pnpm waits for the subprocess's process
 * group too, so a command that was told to stop finishes shutting down
 * before pnpm goes on.
 */
export async function waitForTracked (child: TrackedChild): Promise<NodeJS.Signals | null> {
  const relay = relays.get(child)
  let failure: unknown
  try {
    await child
  } catch (err: unknown) {
    failure = err
  } finally {
    await relay?.settle()
  }
  const signal = relay?.interruptedBy() ?? null
  if (signal == null && failure !== undefined) throw failure
  return signal
}
