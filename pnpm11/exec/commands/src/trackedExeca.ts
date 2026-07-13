import { trackChildProcess } from '@pnpm/exec.lifecycle'
import { safeExeca } from 'execa'

/**
 * `safeExeca`, but the spawned subprocess is registered with the child-process
 * tracker so its process tree can be terminated if pnpm exits on an error while
 * the command is still running. See `trackChildProcess`.
 */
export const trackedExeca = ((...args: Parameters<typeof safeExeca>): ReturnType<typeof safeExeca> => {
  const child = safeExeca(...args)
  trackChildProcess(child)
  return child
}) as typeof safeExeca
