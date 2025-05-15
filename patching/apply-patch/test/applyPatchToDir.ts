import fs from 'fs'
import path from 'path'
import { applyPatchToDir } from '@pnpm/patching.apply-patch'
import { fixtures } from '@pnpm/test-fixtures'
import { tempDir } from '@pnpm/prepare'
import { globalWarn } from '@pnpm/logger'

const f = fixtures(__dirname)

jest.mock('@pnpm/logger', () => {
  const originalModule = jest.requireActual('@pnpm/logger')
  return {
    ...originalModule,
    globalWarn: jest.fn(),
  }
})

beforeEach(() => {
  ;(globalWarn as jest.Mock).mockClear()
})

function prepareDirToPatch () {
  const dir = tempDir()
  f.copy('patch-target.txt', path.join(dir, 'patch-target.txt'))
  return dir
}

describe('applyPatchToDir() without allowFailure', () => {
  const allowFailure = false
  it('should succeed when patch is applicable', () => {
    const patchFilePath = f.find('applicable.patch')
    const successfullyPatched = f.find('successfully-patched.txt')
    const patchedDir = prepareDirToPatch()
    expect(
      applyPatchToDir({
        allowFailure,
        patchFilePath,
        patchedDir,
      })
    ).toBe(true)
    const patchTarget = path.join(patchedDir, 'patch-target.txt')
    expect(fs.readFileSync(patchTarget, 'utf-8')).toBe(fs.readFileSync(successfullyPatched, 'utf-8'))
  })
  it('should fail when patch fails to apply', () => {
    const patchFilePath = f.find('non-applicable.patch')
    const patchedDir = prepareDirToPatch()
    expect(() => {
      applyPatchToDir({
        allowFailure,
        patchFilePath,
        patchedDir,
      })
    }).toThrow(`Could not apply patch ${patchFilePath} to ${patchedDir}`)
    expect(fs.readFileSync(path.join(patchedDir, 'patch-target.txt'), 'utf-8')).toBe(fs.readFileSync(f.find('patch-target.txt'), 'utf-8'))
  })
  it('should fail on invalid patch', () => {
    const patchFilePath = f.find('invalid.patch')
    expect(() => {
      applyPatchToDir({
        allowFailure,
        patchFilePath,
        patchedDir: tempDir(),
      })
    }).toThrow(`Applying patch "${patchFilePath}" failed: hunk header integrity check failed`)
  })
  it('should fail if the patch file is not found', () => {
    expect(() => {
      applyPatchToDir({
        allowFailure,
        patchFilePath: 'does-not-exist.patch',
        patchedDir: tempDir(),
      })
    }).toThrow('Patch file not found')
  })
})

describe('applyPatchToDir() with allowFailure', () => {
  const allowFailure = true
  it('should succeed when patch is applicable', () => {
    const patchFilePath = f.find('applicable.patch')
    const successfullyPatched = f.find('successfully-patched.txt')
    const patchedDir = prepareDirToPatch()
    expect(
      applyPatchToDir({
        allowFailure,
        patchFilePath,
        patchedDir,
      })
    ).toBe(true)
    const patchTarget = path.join(patchedDir, 'patch-target.txt')
    expect(fs.readFileSync(patchTarget, 'utf-8')).toBe(fs.readFileSync(successfullyPatched, 'utf-8'))
  })
  it('should warn when patch fails to apply', () => {
    const patchFilePath = f.find('non-applicable.patch')
    const patchedDir = prepareDirToPatch()
    expect(
      applyPatchToDir({
        allowFailure,
        patchFilePath,
        patchedDir,
      })
    ).toBe(false)
    expect((globalWarn as jest.Mock).mock.calls).toStrictEqual([[
      `Could not apply patch ${patchFilePath} to ${patchedDir}`,
    ]])
    expect(fs.readFileSync(path.join(patchedDir, 'patch-target.txt'), 'utf-8')).toBe(fs.readFileSync(f.find('patch-target.txt'), 'utf-8'))
  })
  it('should fail on invalid patch', () => {
    const patchFilePath = f.find('invalid.patch')
    expect(() => {
      applyPatchToDir({
        allowFailure,
        patchFilePath,
        patchedDir: tempDir(),
      })
    }).toThrow(`Applying patch "${patchFilePath}" failed: hunk header integrity check failed`)
  })
  it('should fail if the patch file is not found', () => {
    expect(() => {
      applyPatchToDir({
        allowFailure,
        patchFilePath: 'does-not-exist.patch',
        patchedDir: tempDir(),
      })
    }).toThrow('Patch file not found')
  })
})
