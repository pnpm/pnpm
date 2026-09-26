import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterEach, expect, jest, test } from '@jest/globals'
import { hoist } from '@pnpm/installing.linking.hoist'
import type { DepPath, ProjectId } from '@pnpm/types'
import { resolveLinkTarget } from 'resolve-link-target'
import { symlinkDir } from 'symlink-dir'

const platform = Object.getOwnPropertyDescriptor(process, 'platform')!

afterEach(() => {
  jest.restoreAllMocks()
  Object.defineProperty(process, 'platform', platform)
})

test.each(Array.from({ length: 100 }, (_, round) => round + 1))('concurrent hoists can replace the same stale dependency link (round %i)', async () => {
  const { root, link, target, opts } = await prepareStaleHoist()
  try {
    const results = await Promise.allSettled(Array.from({ length: 32 }, () => hoist(opts)))
    expect(results.filter(result => result.status === 'rejected')).toStrictEqual([])
    expect(await resolveLinkTarget(link)).toBe(target)
    expect(fs.readFileSync(path.join(link, 'index.js'), 'utf8')).toBe('module.exports = 2')
  } finally {
    fs.rmSync(root, { recursive: true, force: true })
  }
})

test('retries a Windows lock when inspecting a stale hoist link owner', async () => {
  const { root, link, target, opts } = await prepareStaleHoist()
  Object.defineProperty(process, 'platform', { value: 'win32' })
  const readlink = fs.promises.readlink
  let reads = 0
  jest.spyOn(fs.promises, 'readlink').mockImplementation(async (...args) => {
    if (args[0] === link && ++reads === 2) {
      throw Object.assign(new Error('access denied'), { code: 'EPERM' })
    }
    return readlink(...args)
  })
  try {
    await hoist(opts)
    expect(await resolveLinkTarget(link)).toBe(target)
    expect(fs.readFileSync(path.join(link, 'index.js'), 'utf8')).toBe('module.exports = 2')
  } finally {
    fs.rmSync(root, { recursive: true, force: true })
  }
})

test('reports a Windows link-ownership lock that does not clear', async () => {
  const { root, link, opts } = await prepareStaleHoist()
  Object.defineProperty(process, 'platform', { value: 'win32' })
  const readlink = fs.promises.readlink
  let reads = 0
  jest.spyOn(fs.promises, 'readlink').mockImplementation(async (...args) => {
    if (args[0] === link && ++reads >= 2) {
      throw Object.assign(new Error('access denied'), { code: 'EPERM' })
    }
    return readlink(...args)
  })
  try {
    await expect(hoist(opts)).rejects.toMatchObject({ code: 'EPERM' })
  } finally {
    fs.rmSync(root, { recursive: true, force: true })
  }
})

test('preserves a directory occupying a hoist destination', async () => {
  const { root, link, opts } = await prepareStaleHoist()
  await fs.promises.unlink(link)
  fs.mkdirSync(link)
  fs.writeFileSync(path.join(link, 'sentinel'), 'keep')
  try {
    await hoist(opts)
    expect(fs.readFileSync(path.join(link, 'sentinel'), 'utf8')).toBe('keep')
  } finally {
    fs.rmSync(root, { recursive: true, force: true })
  }
})

test('retries a Windows lock when creating a workspace hoist link', async () => {
  const { root, opts } = await prepareStaleHoist()
  const workspace = path.join(root, 'workspace')
  const link = path.join(opts.privateHoistedModulesDir, 'workspace')
  fs.mkdirSync(workspace)
  const workspaceOpts = {
    ...opts,
    hoistedWorkspacePackages: { ['workspace' as ProjectId]: { name: 'workspace', dir: workspace } },
  }
  Object.defineProperty(process, 'platform', { value: 'win32' })
  const symlink = fs.promises.symlink
  let attempts = 0
  jest.spyOn(fs.promises, 'symlink').mockImplementation(async (...args) => {
    if (args[1] === link && attempts++ === 0) {
      throw Object.assign(new Error('file is busy'), { code: 'EBUSY' })
    }
    return symlink(...args)
  })
  try {
    await hoist(workspaceOpts)
    expect(await resolveLinkTarget(link)).toBe(workspace)
    expect(attempts).toBeGreaterThan(1)
  } finally {
    fs.rmSync(root, { recursive: true, force: true })
  }
})

test.each(['same', 'different', 'directory'])('checks a %s replacement created by another installer', async (replacement) => {
  const { root, link, target, opts } = await prepareStaleHoist()
  const winner = replacement === 'same' ? target : path.join(root, 'external')
  fs.mkdirSync(winner, { recursive: true })
  const unlink = fs.promises.unlink
  jest.spyOn(fs.promises, 'unlink').mockImplementationOnce(async (dest) => {
    await unlink(dest)
    if (replacement === 'directory') {
      fs.mkdirSync(link)
      fs.writeFileSync(path.join(link, 'sentinel'), 'keep')
    } else {
      await symlinkDir(winner, link)
    }
  })
  try {
    if (replacement === 'same') {
      await hoist(opts)
    } else {
      await expect(hoist(opts)).rejects.toMatchObject({ code: expect.stringMatching(/^(?:EEXIST|EISDIR)$/) })
    }
    if (replacement === 'directory') {
      expect(fs.lstatSync(link).isSymbolicLink()).toBe(false)
      expect(fs.readFileSync(path.join(link, 'sentinel'), 'utf8')).toBe('keep')
    } else {
      expect(await resolveLinkTarget(link)).toBe(winner)
    }
  } finally {
    fs.rmSync(root, { recursive: true, force: true })
  }
})

test.each(['ENOENT', 'EINVAL'])('rechecks a winning link after readlink reports %s', async (code) => {
  const { root, link, target, opts } = await prepareStaleHoist()
  const unlink = fs.promises.unlink
  const readlink = fs.promises.readlink
  jest.spyOn(fs.promises, 'unlink').mockImplementationOnce(async (dest) => {
    await unlink(dest)
    await symlinkDir(target, link)
    let failures = 0
    jest.spyOn(fs.promises, 'readlink').mockImplementation(async (...args) => {
      if (args[0] === link && failures++ < 2) {
        throw Object.assign(new Error('concurrent unlink'), { code })
      }
      return readlink(...args)
    })
  })
  try {
    await hoist(opts)
    expect(await resolveLinkTarget(link)).toBe(target)
    expect(fs.readFileSync(path.join(link, 'index.js'), 'utf8')).toBe('module.exports = 2')
  } finally {
    fs.rmSync(root, { recursive: true, force: true })
  }
})

test('waits for a Windows junction another installer is creating', async () => {
  const { root, link, target, opts } = await prepareStaleHoist()
  Object.defineProperty(process, 'platform', { value: 'win32' })
  const unlink = fs.promises.unlink
  let junctionCreated!: Promise<void>
  jest.spyOn(fs.promises, 'unlink').mockImplementationOnce(async (dest) => {
    await unlink(dest)
    // A junction is an empty directory until its reparse point is set.
    fs.mkdirSync(link)
    junctionCreated = (async () => {
      await new Promise(resolve => setTimeout(resolve, 20))
      fs.rmdirSync(link)
      await symlinkDir(target, link)
    })()
  })
  try {
    await hoist(opts)
    await junctionCreated
    expect(await resolveLinkTarget(link)).toBe(target)
  } finally {
    fs.rmSync(root, { recursive: true, force: true })
  }
})

test('rechecks a Windows junction completed while its directory is listed', async () => {
  const { root, link, target, opts } = await prepareStaleHoist()
  Object.defineProperty(process, 'platform', { value: 'win32' })
  const unlink = fs.promises.unlink
  const readdir = fs.promises.readdir
  jest.spyOn(fs.promises, 'unlink').mockImplementationOnce(async (dest) => {
    await unlink(dest)
    fs.mkdirSync(link)
    jest.spyOn(fs.promises, 'readdir').mockImplementation(async (...args) => {
      if (args[0] === link && !fs.lstatSync(link).isSymbolicLink()) {
        fs.rmdirSync(link)
        await symlinkDir(target, link)
      }
      return readdir(...args)
    })
  })
  try {
    await hoist(opts)
    expect(await resolveLinkTarget(link)).toBe(target)
  } finally {
    fs.rmSync(root, { recursive: true, force: true })
  }
})

test('stops retrying when a competing link stays unreadable', async () => {
  const { root, link, target, opts } = await prepareStaleHoist()
  const unlink = fs.promises.unlink
  const readlink = fs.promises.readlink
  jest.spyOn(fs.promises, 'unlink').mockImplementationOnce(async (dest) => {
    await unlink(dest)
    await symlinkDir(target, link)
    jest.spyOn(fs.promises, 'readlink').mockImplementation(async (...args) => {
      if (args[0] === link) throw Object.assign(new Error('link is unreadable'), { code: 'ENOENT' })
      return readlink(...args)
    })
  })
  try {
    await expect(hoist(opts)).rejects.toMatchObject({ code: expect.stringMatching(/^(?:EEXIST|EISDIR)$/) })
  } finally {
    fs.rmSync(root, { recursive: true, force: true })
  }
})

test('recreates a link removed before ownership inspection', async () => {
  const { root, link, target, opts } = await prepareStaleHoist()
  const readlink = fs.promises.readlink
  let reads = 0
  jest.spyOn(fs.promises, 'readlink').mockImplementation(async (...args) => {
    if (args[0] === link && ++reads === 2) {
      await fs.promises.unlink(link)
    }
    return readlink(...args)
  })
  try {
    await hoist(opts)
    expect(await resolveLinkTarget(link)).toBe(target)
  } finally {
    fs.rmSync(root, { recursive: true, force: true })
  }
})

async function prepareStaleHoist () {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-concurrent-hoist-'))
  const modulesDir = path.join(root, 'node_modules')
  const virtualStoreDir = path.join(modulesDir, '.pnpm')
  const privateHoistedModulesDir = path.join(virtualStoreDir, 'node_modules')
  const oldTarget = path.join(virtualStoreDir, 'dep@1.0.0/node_modules/dep')
  const target = path.join(virtualStoreDir, 'dep@2.0.0/node_modules/dep')
  fs.mkdirSync(oldTarget, { recursive: true })
  fs.mkdirSync(target, { recursive: true })
  fs.writeFileSync(path.join(target, 'index.js'), 'module.exports = 2')
  const link = path.join(privateHoistedModulesDir, 'dep')
  await symlinkDir(oldTarget, link)

  const opts = {
    graph: {
      parent: {
        dir: path.join(virtualStoreDir, 'parent@1.0.0/node_modules/parent'),
        children: { dep: 'dep' },
        optionalDependencies: new Set<string>(),
        hasBin: false,
        name: 'parent',
        depPath: 'parent@1.0.0' as DepPath,
      },
      dep: {
        dir: target,
        children: {},
        optionalDependencies: new Set<string>(),
        hasBin: false,
        name: 'dep',
        depPath: 'dep@2.0.0' as DepPath,
      },
    },
    directDepsByImporterId: { ['.' as ProjectId]: new Map([['parent', 'parent']]) },
    skipped: new Set<DepPath>(),
    privateHoistPattern: ['*'],
    publicHoistPattern: [],
    privateHoistedModulesDir,
    publicHoistedModulesDir: modulesDir,
    virtualStoreDir,
    virtualStoreDirMaxLength: 120,
  }
  return { root, link, target, opts }
}
