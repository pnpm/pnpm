import { getAvailableParallelism as readAvailableParallelism } from '@pnpm/os.available-parallelism'

const MaxDefaultWorkspaceConcurrency: number = 4

let cacheAvailableParallelism: number | undefined

export function getAvailableParallelism (cache: boolean = true): number {
  if (cache && Number(cacheAvailableParallelism) > 0) {
    return cacheAvailableParallelism!
  }
  cacheAvailableParallelism = readAvailableParallelism()
  return cacheAvailableParallelism
}

export function resetAvailableParallelismCache (): void {
  cacheAvailableParallelism = undefined
}

export function getDefaultWorkspaceConcurrency (cache?: boolean): number {
  return Math.min(MaxDefaultWorkspaceConcurrency, getAvailableParallelism(cache))
}

export function getWorkspaceConcurrency (option: number | undefined): number {
  if (typeof option !== 'number') return getDefaultWorkspaceConcurrency()

  if (option <= 0) {
    // If option is <= 0, it uses the amount of cores minus the absolute of the number given
    // but always returning at least 1
    return Math.max(1, getAvailableParallelism() - Math.abs(option))
  }

  return option
}
