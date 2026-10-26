import fs from 'node:fs'
import path from 'node:path'

interface CgroupMembership {
  version: 1 | 2
  /** The process's cgroup, relative to the hierarchy root, without a leading slash. */
  cgroupPath: string
}

/**
 * The mount points of the cgroup v1 `cpu` controller that cgroups(7) lists.
 * The second is the usual one; the first is a symlink to it on most distributions.
 */
const V1_CPU_CONTROLLER_MOUNTS = ['cpu', 'cpu,cpuacct']

/**
 * The number of CPUs the cgroup CPU quota allows this process, or `undefined`
 * when no quota applies or the cgroup files cannot be read.
 *
 * The quota is the smallest `limit / period` found walking from the process's
 * cgroup up to the hierarchy root, rounded down and floored at 1, as Rust's
 * `std::thread::available_parallelism` computes it. Unlike the kernel-provided
 * CPU count, a container's quota is what the scheduler lets it use.
 *
 * Levels whose directory is missing are skipped rather than aborting the walk.
 * A container runtime that leaves the host's cgroup path in `/proc/self/cgroup`
 * but mounts only the container's own cgroup at `/sys/fs/cgroup` makes the
 * path unreachable, while the files at the mount root are the container's.
 *
 * `rootDir` is the filesystem root the `/proc` and `/sys` trees are read from.
 * Tests pass a fixture tree.
 */
export function readCgroupCpuQuota (rootDir: string = '/'): number | undefined {
  const membership = readTextFile(path.join(rootDir, 'proc/self/cgroup'))
  if (membership == null) return undefined
  const cgroup = parseCgroupMembership(membership)
  if (cgroup == null) return undefined
  const sysFsCgroup = path.join(rootDir, 'sys/fs/cgroup')
  const quota = cgroup.version === 2
    ? minQuotaInHierarchy(sysFsCgroup, cgroup.cgroupPath, readCpuMax)
    : minimum(
      V1_CPU_CONTROLLER_MOUNTS.map((mount) => minQuotaInHierarchy(path.join(sysFsCgroup, mount), cgroup.cgroupPath, readCfsQuota))
    )
  if (quota == null) return undefined
  return Math.max(1, Math.floor(quota))
}

/**
 * Parses `/proc/self/cgroup`, whose lines are `hierarchy-ID:controllers:path`.
 * cgroup v2 has one line with an empty controller list. A cgroup v1 line that
 * names the `cpu` controller wins over the v2 line, as on a hybrid hierarchy
 * the quota lives in the hierarchy that owns the controller.
 */
export function parseCgroupMembership (content: string): CgroupMembership | undefined {
  let membership: CgroupMembership | undefined
  for (const line of content.split('\n')) {
    const firstColon = line.indexOf(':')
    const secondColon = line.indexOf(':', firstColon + 1)
    if (firstColon === -1 || secondColon === -1) continue
    const controllers = line.slice(firstColon + 1, secondColon)
    const cgroupPath = line.slice(secondColon + 1).replace(/^\/+/, '')
    if (controllers.split(',').includes('cpu')) {
      return { version: 1, cgroupPath }
    }
    if (controllers === '' && membership == null) {
      membership = { version: 2, cgroupPath }
    }
  }
  return membership
}

function minQuotaInHierarchy (
  mountDir: string,
  cgroupPath: string,
  readQuota: (dir: string) => number | undefined
): number | undefined {
  const quotas: Array<number | undefined> = []
  for (let dir = path.join(mountDir, cgroupPath); isWithin(mountDir, dir); dir = path.dirname(dir)) {
    quotas.push(readQuota(dir))
  }
  return minimum(quotas)
}

function isWithin (mountDir: string, dir: string): boolean {
  return dir === mountDir || dir.startsWith(mountDir + path.sep)
}

/** cgroup v2: `cpu.max` holds `<limit> <period>` or `max <period>` for no limit. */
function readCpuMax (dir: string): number | undefined {
  const content = readTextFile(path.join(dir, 'cpu.max'))
  if (content == null) return undefined
  const [limit, period] = content.trim().split(/\s+/)
  return quotaOf(limit, period)
}

/** cgroup v1: `cpu.cfs_quota_us` holds the limit, `-1` for no limit, and `cpu.cfs_period_us` the period. */
function readCfsQuota (dir: string): number | undefined {
  const limit = readTextFile(path.join(dir, 'cpu.cfs_quota_us'))
  const period = readTextFile(path.join(dir, 'cpu.cfs_period_us'))
  if (limit == null || period == null) return undefined
  return quotaOf(limit.trim(), period.trim())
}

function quotaOf (limit: string | undefined, period: string | undefined): number | undefined {
  if (limit == null || period == null) return undefined
  const limitValue = parsePositiveInteger(limit)
  const periodValue = parsePositiveInteger(period)
  if (limitValue == null || periodValue == null) return undefined
  return limitValue / periodValue
}

function parsePositiveInteger (text: string): number | undefined {
  if (!/^\d+$/.test(text)) return undefined
  const value = Number(text)
  return value > 0 ? value : undefined
}

function minimum (values: Array<number | undefined>): number | undefined {
  const present = values.filter((value): value is number => value != null)
  return present.length > 0 ? Math.min(...present) : undefined
}

/** Any failure to read a cgroup file means no quota is known from it, so every error is swallowed. */
function readTextFile (filePath: string): string | undefined {
  try {
    return fs.readFileSync(filePath, 'utf8')
  } catch {
    return undefined
  }
}
