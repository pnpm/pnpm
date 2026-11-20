import fs from 'node:fs'
import path from 'node:path'

interface CgroupMembership {
  version: 1 | 2
  /** The process's cgroup, relative to the hierarchy root, without a leading slash. */
  cgroupPath: string
}

/** A mount of the cgroup hierarchy the process's `cpu` controller lives in. */
interface CgroupMount {
  /** Where the hierarchy is mounted, as an absolute path. */
  mountpoint: string
  /** The cgroup of the hierarchy that is visible at `mountpoint`, as `/proc/self/cgroup` spells it. */
  root: string
}

/**
 * The mount points cgroups(7) lists, used when `/proc/self/mountinfo` names
 * no cgroup mount. The v1 `cpu` entry is a symlink to `cpu,cpuacct` on most
 * distributions.
 */
const DEFAULT_MOUNTS: Record<CgroupMembership['version'], string[]> = {
  1: ['/sys/fs/cgroup/cpu', '/sys/fs/cgroup/cpu,cpuacct'],
  2: ['/sys/fs/cgroup'],
}

/**
 * The number of CPUs the cgroup CPU quota allows this process, or `undefined`
 * when no quota applies or the cgroup files cannot be read.
 *
 * The quota is the smallest `limit / period` found walking from the process's
 * cgroup up to the mount root, rounded down and floored at 1, as Rust's
 * `std::thread::available_parallelism` computes it. Unlike the kernel-provided
 * CPU count, a container's quota is what the scheduler lets it use.
 *
 * The process's cgroup directory is found through `/proc/self/mountinfo`, which
 * says which cgroup of the hierarchy each mount shows. A container runtime that
 * leaves the host's cgroup path in `/proc/self/cgroup` but mounts only the
 * container's own cgroup at `/sys/fs/cgroup` is read correctly that way.
 * Without a usable mount entry, the standard mount points are tried with the
 * path as is, skipping levels whose directory is missing.
 *
 * `rootDir` is the filesystem root the `/proc` and `/sys` trees are read from.
 * Tests pass a fixture tree.
 */
export function readCgroupCpuQuota (rootDir: string = '/'): number | undefined {
  const membership = readTextFile(path.join(rootDir, 'proc/self/cgroup'))
  if (membership == null) return undefined
  const cgroup = parseCgroupMembership(membership)
  // A cgroup outside the namespace root shows as `/../<name>`. No mount shows
  // it, so its quota is unknown rather than that of some other cgroup.
  if (cgroup == null || cgroup.cgroupPath.split('/').includes('..')) return undefined
  const mounts = findCgroupMounts(readTextFile(path.join(rootDir, 'proc/self/mountinfo')), cgroup)
  const readQuota = cgroup.version === 2 ? readCpuMax : readCfsQuota
  const quota = minimum(mounts.map((mount) => minQuotaInHierarchy(rootDir, mount, cgroup.cgroupPath, readQuota)))
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

/**
 * The mounts of the hierarchy that holds the process's `cpu` controller, from
 * `/proc/self/mountinfo` (see proc_pid_mountinfo(5)). Falls back to
 * `DEFAULT_MOUNTS` rooted at the hierarchy root when the file names none.
 */
export function findCgroupMounts (mountinfo: string | undefined, cgroup: CgroupMembership): CgroupMount[] {
  const mounts = (mountinfo ?? '').split('\n').flatMap((line) => parseMountinfoLine(line, cgroup.version) ?? [])
  if (mounts.length > 0) return mounts
  return DEFAULT_MOUNTS[cgroup.version].map((mountpoint) => ({ mountpoint, root: '/' }))
}

/**
 * A mountinfo line is `id parent major:minor root mountpoint options [tags...] - fstype source superopts`.
 * Returns the mount when its filesystem is the cgroup hierarchy of `version`
 * that carries the `cpu` controller.
 */
function parseMountinfoLine (line: string, version: CgroupMembership['version']): CgroupMount | undefined {
  const separator = line.indexOf(' - ')
  if (separator === -1) return undefined
  const [, , , root, mountpoint] = line.slice(0, separator).split(' ')
  const [fstype, , superopts = ''] = line.slice(separator + 3).split(' ')
  if (root == null || mountpoint == null) return undefined
  const matches = version === 2
    ? fstype === 'cgroup2'
    : fstype === 'cgroup' && superopts.split(',').includes('cpu')
  if (!matches) return undefined
  return { mountpoint: unescapeMountinfo(mountpoint), root: unescapeMountinfo(root) }
}

/** mountinfo spells space, tab, newline, and backslash in paths as octal escapes. */
function unescapeMountinfo (field: string): string {
  return field.replace(/\\([0-7]{3})/g, (_, octal: string) => String.fromCharCode(parseInt(octal, 8)))
}

function minQuotaInHierarchy (
  rootDir: string,
  mount: CgroupMount,
  cgroupPath: string,
  readQuota: (dir: string) => number | undefined
): number | undefined {
  const relativePath = relativeCgroupPath(mount.root, cgroupPath)
  if (relativePath == null) return undefined
  const mountDir = path.join(rootDir, mount.mountpoint)
  const quotas: Array<number | undefined> = []
  for (let dir = path.join(mountDir, relativePath); isWithin(mountDir, dir); dir = path.dirname(dir)) {
    quotas.push(readQuota(dir))
  }
  return minimum(quotas)
}

/**
 * The process's cgroup relative to the cgroup the mount shows, or `undefined`
 * when the process is not under it. A root of only `..` segments, such as
 * `/..`, is a mount made before the cgroup namespace (see
 * cgroup_namespaces(7)); it shows an ancestor at an unknown depth, so only the
 * mount root is read. Its quota bounds every descendant, the process's cgroup
 * included. A root that descends again after `..`, such as `/../sibling`, shows
 * a cgroup the process is not under.
 */
function relativeCgroupPath (mountRoot: string, cgroupPath: string): string | undefined {
  const rootSegments = mountRoot.split('/').filter((segment) => segment !== '')
  if (rootSegments.includes('..')) {
    return rootSegments.every((segment) => segment === '..') ? '' : undefined
  }
  const relativePath = path.posix.relative(mountRoot, '/' + cgroupPath)
  return relativePath.split('/')[0] === '..' ? undefined : relativePath
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
