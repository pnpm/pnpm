import os from 'os'

export function getWorkspaceConcurrency (option: number | undefined): number {
  if (typeof option !== 'number') return 4

  if (option <= 0) {
    // If option is <= 0, it uses the amount of cores minus the absolute of the number given
    // but always returning at least 1
    return Math.max(1, (os.availableParallelism?.() ?? os.cpus().length) - Math.abs(option))
  }

  return option
}
