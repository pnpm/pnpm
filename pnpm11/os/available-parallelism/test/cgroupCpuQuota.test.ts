import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterEach, expect, test } from '@jest/globals'

import { parseCgroupMembership, readCgroupCpuQuota } from '../lib/cgroupCpuQuota.js'

const roots: string[] = []

afterEach(() => {
  for (const root of roots.splice(0)) {
    fs.rmSync(root, { recursive: true, force: true })
  }
})

function fixtureRoot (files: Record<string, string>): string {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-cgroup-'))
  roots.push(root)
  for (const [relativePath, content] of Object.entries(files)) {
    const filePath = path.join(root, relativePath)
    fs.mkdirSync(path.dirname(filePath), { recursive: true })
    fs.writeFileSync(filePath, content)
  }
  return root
}

test('no /proc/self/cgroup means no quota', () => {
  expect(readCgroupCpuQuota(fixtureRoot({}))).toBeUndefined()
})

test('cgroup v2: cpu.max of the process cgroup', () => {
  const root = fixtureRoot({
    'proc/self/cgroup': '0::/kubepods/pod1/container1\n',
    'sys/fs/cgroup/kubepods/pod1/container1/cpu.max': '400000 100000\n',
  })
  expect(readCgroupCpuQuota(root)).toBe(4)
})

test('cgroup v2: the smallest quota in the hierarchy wins', () => {
  const root = fixtureRoot({
    'proc/self/cgroup': '0::/kubepods/pod1/container1\n',
    'sys/fs/cgroup/kubepods/cpu.max': '200000 100000\n',
    'sys/fs/cgroup/kubepods/pod1/cpu.max': 'max 100000\n',
    'sys/fs/cgroup/kubepods/pod1/container1/cpu.max': '800000 100000\n',
  })
  expect(readCgroupCpuQuota(root)).toBe(2)
})

test('cgroup v2: "max" is no quota', () => {
  const root = fixtureRoot({
    'proc/self/cgroup': '0::/user.slice\n',
    'sys/fs/cgroup/user.slice/cpu.max': 'max 100000\n',
  })
  expect(readCgroupCpuQuota(root)).toBeUndefined()
})

test('cgroup v2: a fractional quota rounds down to at least 1', () => {
  expect(readCgroupCpuQuota(fixtureRoot({
    'proc/self/cgroup': '0::/\n',
    'sys/fs/cgroup/cpu.max': '250000 100000\n',
  }))).toBe(2)
  expect(readCgroupCpuQuota(fixtureRoot({
    'proc/self/cgroup': '0::/\n',
    'sys/fs/cgroup/cpu.max': '50000 100000\n',
  }))).toBe(1)
})

test('cgroup v2: the mount root is read when the host cgroup path is not mounted in the container', () => {
  const root = fixtureRoot({
    'proc/self/cgroup': '0::/system.slice/docker-abc.scope\n',
    'sys/fs/cgroup/cpu.max': '300000 100000\n',
  })
  expect(readCgroupCpuQuota(root)).toBe(3)
})

test('cgroup v1: cfs quota and period of the cpu,cpuacct controller', () => {
  const root = fixtureRoot({
    'proc/self/cgroup': '12:memory:/docker/abc\n4:cpu,cpuacct:/docker/abc\n1:name=systemd:/docker/abc\n',
    'sys/fs/cgroup/cpu,cpuacct/docker/abc/cpu.cfs_quota_us': '400000\n',
    'sys/fs/cgroup/cpu,cpuacct/docker/abc/cpu.cfs_period_us': '100000\n',
  })
  expect(readCgroupCpuQuota(root)).toBe(4)
})

test('cgroup v1: -1 is no quota', () => {
  const root = fixtureRoot({
    'proc/self/cgroup': '4:cpu,cpuacct:/docker/abc\n',
    'sys/fs/cgroup/cpu,cpuacct/docker/abc/cpu.cfs_quota_us': '-1\n',
    'sys/fs/cgroup/cpu,cpuacct/docker/abc/cpu.cfs_period_us': '100000\n',
  })
  expect(readCgroupCpuQuota(root)).toBeUndefined()
})

test('cgroup v1: the controller mount root is read when the cgroup path is not mounted in the container', () => {
  const root = fixtureRoot({
    'proc/self/cgroup': '4:cpu,cpuacct:/docker/abc\n',
    'sys/fs/cgroup/cpu,cpuacct/cpu.cfs_quota_us': '150000\n',
    'sys/fs/cgroup/cpu,cpuacct/cpu.cfs_period_us': '100000\n',
  })
  expect(readCgroupCpuQuota(root)).toBe(1)
})

test('cgroup v1: the cpu mount point without cpuacct', () => {
  const root = fixtureRoot({
    'proc/self/cgroup': '4:cpu:/ci\n',
    'sys/fs/cgroup/cpu/ci/cpu.cfs_quota_us': '600000\n',
    'sys/fs/cgroup/cpu/ci/cpu.cfs_period_us': '100000\n',
  })
  expect(readCgroupCpuQuota(root)).toBe(6)
})

test('hybrid hierarchy: the v1 cpu controller wins over the v2 line', () => {
  const root = fixtureRoot({
    'proc/self/cgroup': '0::/init.scope\n4:cpu,cpuacct:/docker/abc\n',
    'sys/fs/cgroup/init.scope/cpu.max': '800000 100000\n',
    'sys/fs/cgroup/cpu,cpuacct/docker/abc/cpu.cfs_quota_us': '200000\n',
    'sys/fs/cgroup/cpu,cpuacct/docker/abc/cpu.cfs_period_us': '100000\n',
  })
  expect(readCgroupCpuQuota(root)).toBe(2)
})

test('malformed quota files are ignored', () => {
  const root = fixtureRoot({
    'proc/self/cgroup': '0::/a\n',
    'sys/fs/cgroup/a/cpu.max': 'garbage\n',
    'sys/fs/cgroup/cpu.max': '100000 0\n',
  })
  expect(readCgroupCpuQuota(root)).toBeUndefined()
})

test('parseCgroupMembership', () => {
  expect(parseCgroupMembership('0::/a/b\n')).toEqual({ version: 2, cgroupPath: 'a/b' })
  expect(parseCgroupMembership('0::/\n')).toEqual({ version: 2, cgroupPath: '' })
  expect(parseCgroupMembership('3:cpuacct,cpu:/x\n')).toEqual({ version: 1, cgroupPath: 'x' })
  expect(parseCgroupMembership('3:memory:/x\n')).toBeUndefined()
  expect(parseCgroupMembership('')).toBeUndefined()
})
