import { relaySignals, spawnsInOwnProcessGroup } from '@pnpm/exec.npm-lifecycle'
import spawn from 'cross-spawn'

export interface SpawnPnpmResult {
  status: number | null
  signal: NodeJS.Signals | null
}

/**
 * Run a pnpm executable with this process's stdio and wait for it.
 *
 * Not `spawn.sync`: that blocks the event loop, so a signal sent to this pnpm
 * would sit undelivered until the other pnpm had finished. The signals are
 * relayed to it instead, and this pnpm waits for it to shut down
 * (pnpm/pnpm#9948). Without a controlling terminal the other pnpm leads a
 * process group of its own, which the relay signals as a group, so the
 * scripts it started are reached even if that pnpm does not pass a signal on.
 *
 * Rejects when the executable cannot be started.
 */
export async function spawnPnpm (
  pnpmBinPath: string,
  args: string[],
  opts?: { env?: NodeJS.ProcessEnv }
): Promise<SpawnPnpmResult> {
  const ownProcessGroup = spawnsInOwnProcessGroup()
  const child = spawn(pnpmBinPath, args, {
    stdio: 'inherit',
    env: opts?.env,
    detached: ownProcessGroup,
  })
  const relay = relaySignals(child, { ownProcessGroup, terminateOnExit: true })
  try {
    return await new Promise<SpawnPnpmResult>((resolve, reject) => {
      child.once('error', reject)
      child.once('close', (status, signal) => {
        resolve({ status, signal })
      })
    })
  } finally {
    await relay.settle()
  }
}
