import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { tempDir } from '@pnpm/prepare'

import { resolvePatchDir } from '../src/resolvePatchDir.js'
import { writeEditDirState } from '../src/stateFile.js'

test('resolvePatchDir resolves direct path', async () => {
  const dir = tempDir()
  const modulesDir = path.join(dir, 'node_modules')
  const editDir = path.join(modulesDir, '.pnpm_patches', 'is-positive@1.0.0')
  fs.mkdirSync(editDir, { recursive: true })
  fs.writeFileSync(path.join(editDir, 'package.json'), JSON.stringify({ name: 'is-positive', version: '1.0.0' }))

  writeEditDirState({
    editDir,
    modulesDir,
    patchedPkg: 'is-positive@1.0.0',
    applyToAll: false,
  })

  const resolved = await resolvePatchDir(editDir, {
    dir,
    lockfileDir: dir,
    modulesDir,
  })

  expect(resolved.editDir).toBe(editDir)
  expect(resolved.stateValue.patchedPkg).toBe('is-positive@1.0.0')
})

test('resolvePatchDir resolves by package specifier', async () => {
  const dir = tempDir()
  const modulesDir = path.join(dir, 'node_modules')
  const editDir = path.join(modulesDir, '.pnpm_patches', 'is-positive@1.0.0')
  fs.mkdirSync(editDir, { recursive: true })
  fs.writeFileSync(path.join(editDir, 'package.json'), JSON.stringify({ name: 'is-positive', version: '1.0.0' }))

  writeEditDirState({
    editDir,
    modulesDir,
    patchedPkg: 'is-positive',
    applyToAll: true,
  })

  const resolved = await resolvePatchDir('is-positive@1.0.0', {
    dir,
    lockfileDir: dir,
    modulesDir,
  })

  expect(resolved.editDir).toBe(editDir)
  expect(resolved.stateValue.applyToAll).toBe(true)
})

test('resolvePatchDir resolves by package name alone', async () => {
  const dir = tempDir()
  const modulesDir = path.join(dir, 'node_modules')
  const editDir = path.join(modulesDir, '.pnpm_patches', 'is-positive@1.0.0')
  fs.mkdirSync(editDir, { recursive: true })
  fs.writeFileSync(path.join(editDir, 'package.json'), JSON.stringify({ name: 'is-positive', version: '1.0.0' }))

  writeEditDirState({
    editDir,
    modulesDir,
    patchedPkg: 'is-positive',
    applyToAll: true,
  })

  const resolved = await resolvePatchDir('is-positive', {
    dir,
    lockfileDir: dir,
    modulesDir,
  })

  expect(resolved.editDir).toBe(editDir)
})

test('resolvePatchDir resolves scoped package', async () => {
  const dir = tempDir()
  const modulesDir = path.join(dir, 'node_modules')
  const editDir = path.join(modulesDir, '.pnpm_patches', '@scope', 'pkg@2.0.0')
  fs.mkdirSync(editDir, { recursive: true })
  fs.writeFileSync(path.join(editDir, 'package.json'), JSON.stringify({ name: '@scope/pkg', version: '2.0.0' }))

  writeEditDirState({
    editDir,
    modulesDir,
    patchedPkg: '@scope/pkg@2.0.0',
    applyToAll: false,
  })

  const resolvedByName = await resolvePatchDir('@scope/pkg', {
    dir,
    lockfileDir: dir,
    modulesDir,
  })
  expect(resolvedByName.editDir).toBe(editDir)

  const resolvedBySpec = await resolvePatchDir('@scope/pkg@2.0.0', {
    dir,
    lockfileDir: dir,
    modulesDir,
  })
  expect(resolvedBySpec.editDir).toBe(editDir)
})

test('resolvePatchDir throws ambiguous error when multiple versions exist and bare name is passed', async () => {
  const dir = tempDir()
  const modulesDir = path.join(dir, 'node_modules')
  const editDir1 = path.join(modulesDir, '.pnpm_patches', 'is-positive@1.0.0')
  const editDir2 = path.join(modulesDir, '.pnpm_patches', 'is-positive@2.0.0')
  fs.mkdirSync(editDir1, { recursive: true })
  fs.mkdirSync(editDir2, { recursive: true })
  fs.writeFileSync(path.join(editDir1, 'package.json'), JSON.stringify({ name: 'is-positive', version: '1.0.0' }))
  fs.writeFileSync(path.join(editDir2, 'package.json'), JSON.stringify({ name: 'is-positive', version: '2.0.0' }))

  writeEditDirState({
    editDir: editDir1,
    modulesDir,
    patchedPkg: 'is-positive@1.0.0',
    applyToAll: false,
  })
  writeEditDirState({
    editDir: editDir2,
    modulesDir,
    patchedPkg: 'is-positive@2.0.0',
    applyToAll: false,
  })

  await expect(resolvePatchDir('is-positive', {
    dir,
    lockfileDir: dir,
    modulesDir,
  })).rejects.toMatchObject({
    code: 'ERR_PNPM_AMBIGUOUS_PATCH_TARGET',
  })

  const resolved = await resolvePatchDir('is-positive@1.0.0', {
    dir,
    lockfileDir: dir,
    modulesDir,
  })
  expect(resolved.editDir).toBe(editDir1)
})

test('resolvePatchDir throws ambiguous error when one state entry has bare name and another has versioned specifier', async () => {
  const dir = tempDir()
  const modulesDir = path.join(dir, 'node_modules')
  const editDir1 = path.join(modulesDir, '.pnpm_patches/is-positive@1.0.0')
  const editDir2 = path.join(modulesDir, '.pnpm_patches/is-positive@2.0.0')

  fs.mkdirSync(editDir1, { recursive: true })
  fs.writeFileSync(path.join(editDir1, 'package.json'), JSON.stringify({ name: 'is-positive', version: '1.0.0' }))
  writeEditDirState({
    modulesDir,
    editDir: editDir1,
    patchedPkg: 'is-positive',
    applyToAll: true,
  })

  fs.mkdirSync(editDir2, { recursive: true })
  fs.writeFileSync(path.join(editDir2, 'package.json'), JSON.stringify({ name: 'is-positive', version: '2.0.0' }))
  writeEditDirState({
    modulesDir,
    editDir: editDir2,
    patchedPkg: 'is-positive@2.0.0',
    applyToAll: false,
  })

  await expect(resolvePatchDir('is-positive', {
    dir,
    lockfileDir: dir,
    modulesDir,
  })).rejects.toMatchObject({
    code: 'ERR_PNPM_AMBIGUOUS_PATCH_TARGET',
  })
})

test('resolvePatchDir throws invalid patch dir when no matches found', async () => {
  const dir = tempDir()
  const modulesDir = path.join(dir, 'node_modules')

  await expect(resolvePatchDir('non-existent', {
    dir,
    lockfileDir: dir,
    modulesDir,
  })).rejects.toMatchObject({
    code: 'ERR_PNPM_INVALID_PATCH_DIR',
  })
})
