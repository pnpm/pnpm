import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterEach, beforeEach, expect, jest, test } from '@jest/globals'

import type { PatchRemoveCommandOptions } from '../src/patchRemove.js'

jest.unstable_mockModule('@pnpm/installing.commands', () => ({
  install: {
    handler: jest.fn(),
  },
}))

jest.unstable_mockModule('../src/updatePatchedDependencies.js', () => ({
  updatePatchedDependencies: jest.fn(),
}))

const { install } = await import('@pnpm/installing.commands')
const patchRemove = await import('../src/patchRemove.js')
const { updatePatchedDependencies } = await import('../src/updatePatchedDependencies.js')

const installHandler = jest.mocked(install.handler)
const updatePatchedDependenciesMock = jest.mocked(updatePatchedDependencies)
const testOnNonWindows = process.platform === 'win32' ? test.skip : test

let tempRoot: string

beforeEach(() => {
  tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-patch-remove-'))
  installHandler.mockResolvedValue(undefined)
  updatePatchedDependenciesMock.mockResolvedValue(undefined)
})

afterEach(() => {
  installHandler.mockReset()
  updatePatchedDependenciesMock.mockReset()
  fs.rmSync(tempRoot, { force: true, recursive: true })
})

test('patch-remove rejects traversal outside the patches directory before deleting any patch', async () => {
  const projectDir = path.join(tempRoot, 'project')
  const outsideFile = path.join(tempRoot, 'outside.patch')
  const goodPatch = path.join(projectDir, 'patches/good.patch')
  fs.mkdirSync(path.dirname(goodPatch), { recursive: true })
  fs.writeFileSync(goodPatch, 'good patch', 'utf8')
  fs.writeFileSync(outsideFile, 'outside patch', 'utf8')

  await expect(patchRemove.handler(createOptions(projectDir, {
    good: 'patches/good.patch',
    bad: '../outside.patch',
  }), ['good', 'bad'])).rejects.toMatchObject({
    code: 'ERR_PNPM_PATCH_FILE_OUTSIDE_PATCHES_DIR',
  })

  expect(fs.existsSync(goodPatch)).toBe(true)
  expect(fs.existsSync(outsideFile)).toBe(true)
  expect(updatePatchedDependenciesMock).not.toHaveBeenCalled()
  expect(installHandler).not.toHaveBeenCalled()
})

test('patch-remove rejects directory entries before deleting any patch', async () => {
  const projectDir = path.join(tempRoot, 'project')
  const goodPatch = path.join(projectDir, 'patches/good.patch')
  const patchDir = path.join(projectDir, 'patches/not-a-file.patch')
  fs.mkdirSync(patchDir, { recursive: true })
  fs.writeFileSync(goodPatch, 'good patch', 'utf8')

  await expect(patchRemove.handler(createOptions(projectDir, {
    good: 'patches/good.patch',
    bad: 'patches/not-a-file.patch',
  }), ['good', 'bad'])).rejects.toMatchObject({
    code: 'ERR_PNPM_PATCH_FILE_IS_DIRECTORY',
  })

  expect(fs.existsSync(goodPatch)).toBe(true)
  expect(updatePatchedDependenciesMock).not.toHaveBeenCalled()
  expect(installHandler).not.toHaveBeenCalled()
})

testOnNonWindows('patch-remove rejects a nested parent symlink outside the patches directory before unlinking a dangling target', async () => {
  const projectDir = path.join(tempRoot, 'project')
  const patchesDir = path.join(projectDir, 'patches')
  const outsideDir = path.join(tempRoot, 'outside')
  const outsideDanglingLink = path.join(outsideDir, 'dangling.patch')
  fs.mkdirSync(patchesDir, { recursive: true })
  fs.mkdirSync(outsideDir, { recursive: true })
  fs.symlinkSync(outsideDir, path.join(patchesDir, 'linked-dir'), 'dir')
  fs.symlinkSync(path.join(tempRoot, 'missing-target.patch'), outsideDanglingLink)

  await expect(patchRemove.handler(createOptions(projectDir, {
    bad: 'patches/linked-dir/dangling.patch',
  }), ['bad'])).rejects.toMatchObject({
    code: 'ERR_PNPM_PATCH_FILE_OUTSIDE_PATCHES_DIR',
  })

  expect(fs.lstatSync(outsideDanglingLink).isSymbolicLink()).toBe(true)
  expect(updatePatchedDependenciesMock).not.toHaveBeenCalled()
  expect(installHandler).not.toHaveBeenCalled()
})

testOnNonWindows('patch-remove unlinks a final symlink inside the patches directory without touching its target', async () => {
  const projectDir = path.join(tempRoot, 'project')
  const patchesDir = path.join(projectDir, 'patches')
  const outsideTarget = path.join(tempRoot, 'outside-target.patch')
  const patchLink = path.join(patchesDir, 'linked.patch')
  fs.mkdirSync(patchesDir, { recursive: true })
  fs.writeFileSync(outsideTarget, 'outside target', 'utf8')
  fs.symlinkSync(outsideTarget, patchLink)

  await patchRemove.handler(createOptions(projectDir, {
    pkg: 'patches/linked.patch',
  }), ['pkg'])

  expect(fs.existsSync(patchLink)).toBe(false)
  expect(fs.readFileSync(outsideTarget, 'utf8')).toBe('outside target')
  expect(updatePatchedDependenciesMock).toHaveBeenCalledWith({}, expect.any(Object))
  expect(installHandler).toHaveBeenCalledWith(expect.objectContaining({
    patchedDependencies: {},
  }))
})

test('patch-remove allows a symlinked patches directory that resolves inside the project', async () => {
  const projectDir = path.join(tempRoot, 'project')
  const realPatchesDir = path.join(projectDir, 'real-patches')
  const patchFile = path.join(realPatchesDir, 'pkg.patch')
  fs.mkdirSync(realPatchesDir, { recursive: true })
  fs.symlinkSync(realPatchesDir, path.join(projectDir, 'patches'), process.platform === 'win32' ? 'junction' : 'dir')
  fs.writeFileSync(patchFile, 'patch', 'utf8')

  await patchRemove.handler(createOptions(projectDir, {
    pkg: 'patches/pkg.patch',
  }), ['pkg'])

  expect(fs.existsSync(patchFile)).toBe(false)
  expect(updatePatchedDependenciesMock).toHaveBeenCalledWith({}, expect.any(Object))
  expect(installHandler).toHaveBeenCalledWith(expect.objectContaining({
    patchedDependencies: {},
  }))
})

test('patch-remove keeps missing patch files as no-ops', async () => {
  const projectDir = path.join(tempRoot, 'project')
  fs.mkdirSync(path.join(projectDir, 'patches'), { recursive: true })

  await patchRemove.handler(createOptions(projectDir, {
    pkg: 'patches/missing.patch',
  }), ['pkg'])

  expect(updatePatchedDependenciesMock).toHaveBeenCalledWith({}, expect.any(Object))
  expect(installHandler).toHaveBeenCalledWith(expect.objectContaining({
    patchedDependencies: {},
  }))
})

function createOptions (
  projectDir: string,
  patchedDependencies: Record<string, string>
): PatchRemoveCommandOptions {
  return {
    dir: projectDir,
    lockfileDir: projectDir,
    patchedDependencies,
    rootProjectManifest: {},
    rootProjectManifestDir: projectDir,
  } as PatchRemoveCommandOptions
}
