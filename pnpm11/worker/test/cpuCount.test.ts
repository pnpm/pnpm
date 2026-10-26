import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterEach, expect, test } from '@jest/globals'

const { getAvailableParallelism } = await import('../lib/cpuCount.js')
const temporaryDirectories: string[] = []
const linuxTest = process.platform === 'linux' ? test : test.skip

afterEach(() => {
  for (const directory of temporaryDirectories.splice(0)) {
    fs.rmSync(directory, { recursive: true, force: true })
  }
})

linuxTest('uses the most restrictive cgroup v2 quota in the hierarchy', () => {
  const fixture = createFixture()
  const mountPoint = path.join(fixture, 'sys', 'fs', 'cgroup mount')
  const encodedMountPoint = mountPoint.replaceAll(' ', '\\040')
  writeCgroupFiles(fixture, '0::/team/job\n', `36 25 0:32 /team ${encodedMountPoint} rw - cgroup2 cgroup rw\n`)
  writeQuota(mountPoint, '250000 100000\n')
  writeQuota(path.join(mountPoint, 'job'), 'max 100000\n')

  expect(getAvailableParallelism(233, path.join(fixture, 'proc'))).toBe(3)
})

linuxTest('maps cgroup v1 paths and inherits ancestor quotas', () => {
  const fixture = createFixture()
  const mountPoint = path.join(fixture, 'sys', 'fs', 'cpu')
  writeCgroupFiles(fixture, '5:cpu,cpuacct:/docker/job\n', `36 25 0:32 /docker ${mountPoint} rw - cgroup cgroup rw,cpu,cpuacct\n`)
  writeV1Quota(mountPoint, '500000', '100000')
  writeV1Quota(path.join(mountPoint, 'job'), '-1', '100000')

  expect(getAvailableParallelism(233, path.join(fixture, 'proc'))).toBe(5)
})

linuxTest('falls back to CPU affinity when cgroup quota data is unavailable', () => {
  const fixture = createFixture()
  writeCgroupFiles(fixture, '0::/team/job\n', '36 25 0:32 / /sys/fs/cgroup rw - cgroup2 cgroup rw\n')

  expect(getAvailableParallelism(233, path.join(fixture, 'proc'))).toBe(233)
})

function createFixture (): string {
  const fixture = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-cpu-count-'))
  temporaryDirectories.push(fixture)
  return fixture
}

function writeCgroupFiles (fixture: string, cgroup: string, mountinfo: string): void {
  const procDir = path.join(fixture, 'proc', 'self')
  fs.mkdirSync(procDir, { recursive: true })
  fs.writeFileSync(path.join(procDir, 'cgroup'), cgroup)
  fs.writeFileSync(path.join(procDir, 'mountinfo'), mountinfo)
}

function writeQuota (directory: string, quota: string): void {
  fs.mkdirSync(directory, { recursive: true })
  fs.writeFileSync(path.join(directory, 'cpu.max'), quota)
}

function writeV1Quota (directory: string, quota: string, period: string): void {
  fs.mkdirSync(directory, { recursive: true })
  fs.writeFileSync(path.join(directory, 'cpu.cfs_quota_us'), quota)
  fs.writeFileSync(path.join(directory, 'cpu.cfs_period_us'), period)
}
