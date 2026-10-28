import os from 'node:os'

import { readCgroupCpuQuota } from './cgroupCpuQuota.js'

export { readCgroupCpuQuota } from './cgroupCpuQuota.js'

/**
 * The number of CPUs this process may use, at least 1.
 *
 * Starts from `os.availableParallelism()`, which counts the CPUs in the
 * process's affinity mask. On Linux, a cgroup CPU quota smaller than that
 * count wins, so a container limited to 4 CPUs on a 200-CPU host reports 4.
 */
export function getAvailableParallelism (): number {
  const cpus = Math.max(1, os.availableParallelism())
  if (process.platform !== 'linux') return cpus
  const quota = readCgroupCpuQuota()
  return quota == null ? cpus : Math.min(cpus, quota)
}
