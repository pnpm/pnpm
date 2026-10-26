import { readFileSync } from 'node:fs'
import path from 'node:path'

interface CgroupPath {
  type: 'v1' | 'v2'
  path: string
}

interface CgroupMount {
  mountPoint: string
  path: string
  type: CgroupPath['type']
}

export function getAvailableParallelism (affinityCount: number, procDir: string = '/proc'): number {
  if (process.platform !== 'linux') return affinityCount
  const cgroup = findCgroup(readFileOrEmpty(path.posix.join(procDir, 'self/cgroup')))
  if (!cgroup) return affinityCount
  const mount = findCgroupMount(readFileOrEmpty(path.posix.join(procDir, 'self/mountinfo')), cgroup)
  if (!mount) return affinityCount
  const quota = getQuotaInHierarchy(mount)
  if (quota === undefined) return affinityCount
  return Math.min(affinityCount, Math.max(1, Math.ceil(quota)))
}

function findCgroup (contents: string): CgroupPath | undefined {
  let unifiedCgroup: CgroupPath | undefined
  for (const line of contents.split('\n')) {
    const [hierarchy, controllers, cgroupPath] = line.split(':', 3)
    if (cgroupPath === undefined) continue
    if (hierarchy === '0' && controllers === '') unifiedCgroup = { type: 'v2', path: cgroupPath }
    if (controllers?.split(',').includes('cpu')) return { type: 'v1', path: cgroupPath }
  }
  return unifiedCgroup
}

function findCgroupMount (contents: string, cgroup: CgroupPath): CgroupMount | undefined {
  for (const line of contents.split('\n')) {
    const mount = parseCgroupMount(line, cgroup)
    if (mount) return mount
  }
  return undefined
}

function parseCgroupMount (line: string, cgroup: CgroupPath): CgroupMount | undefined {
  const separator = line.lastIndexOf(' - ')
  if (separator < 0) return undefined
  const mountFields = line.slice(0, separator).split(' ')
  const filesystemFields = line.slice(separator + 3).split(' ')
  if (!isCgroupFilesystem(filesystemFields, cgroup.type)) return undefined
  const root = decodeMountPath(mountFields[3] ?? '')
  const mountPoint = decodeMountPath(mountFields[4] ?? '')
  const relativePath = relativeCgroupPath(root, cgroup.path)
  if (relativePath === undefined || mountPoint === '') return undefined
  return { mountPoint, path: path.posix.join(mountPoint, relativePath), type: cgroup.type }
}

function isCgroupFilesystem (fields: string[], type: CgroupPath['type']): boolean {
  if (type === 'v2') return fields[0] === 'cgroup2'
  return fields[0] === 'cgroup' && Boolean(fields[2]?.split(',').includes('cpu'))
}

function relativeCgroupPath (root: string, cgroupPath: string): string | undefined {
  const normalizedRoot = path.posix.normalize(root)
  const normalizedCgroupPath = path.posix.normalize(cgroupPath)
  if (normalizedRoot === normalizedCgroupPath) return ''
  if (normalizedRoot === '/') return normalizedCgroupPath.slice(1)
  if (normalizedCgroupPath.startsWith(`${normalizedRoot}/`)) return normalizedCgroupPath.slice(normalizedRoot.length + 1)
  return undefined
}

function getQuotaInHierarchy (mount: CgroupMount): number | undefined {
  let directory = mount.path
  let quota: number | undefined
  while (true) {
    const currentQuota = readQuota(directory, mount.type)
    if (currentQuota !== undefined) quota = quota === undefined ? currentQuota : Math.min(quota, currentQuota)
    if (directory === mount.mountPoint) return quota
    const parent = path.posix.dirname(directory)
    if (parent === directory) return quota
    directory = parent
  }
}

function readQuota (directory: string, type: CgroupPath['type']): number | undefined {
  if (type === 'v2') {
    const [quotaValue, periodValue] = readFileOrEmpty(path.posix.join(directory, 'cpu.max')).trim().split(/\s+/)
    if (quotaValue === 'max') return
    return parseQuota(quotaValue, periodValue)
  }
  const quotaValue = readFileOrEmpty(path.posix.join(directory, 'cpu.cfs_quota_us')).trim()
  const periodValue = readFileOrEmpty(path.posix.join(directory, 'cpu.cfs_period_us')).trim()
  return parseQuota(quotaValue, periodValue)
}

function parseQuota (quotaValue: string | undefined, periodValue: string | undefined): number | undefined {
  if (!quotaValue || !periodValue || !/^-?\d+$/.test(quotaValue) || !/^\d+$/.test(periodValue)) return
  const quota = Number(quotaValue)
  const period = Number(periodValue)
  if (!Number.isSafeInteger(quota) || !Number.isSafeInteger(period) || quota <= 0 || period <= 0) return
  return quota / period
}

function readFileOrEmpty (file: string): string {
  try {
    return readFileSync(file, 'utf8')
  } catch {
    return ''
  }
}

function decodeMountPath (mountPath: string): string {
  return mountPath.replace(/\\([0-7]{3})/g, (_, octal: string) => String.fromCharCode(parseInt(octal, 8)))
}
