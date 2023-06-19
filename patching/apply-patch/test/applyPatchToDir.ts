import { applyPatchToDir } from '@pnpm/patching.apply-patch'
import { fixtures } from '@pnpm/test-fixtures'
import { tempDir } from '@pnpm/prepare'

const f = fixtures(__dirname)

describe('applyPatchToDir()', () => {
  it('should fail on invalid patch', () => {
    const patchFilePath = f.find('invalid.patch')
    expect(() => {
      applyPatchToDir({
        patchFilePath,
        patchedDir: tempDir(),
      })
    }).toThrowError(`Applying patch "${patchFilePath}" failed: hunk header integrity check failed`)
  })
  it('should fail if the patch file is not found', () => {
    expect(() => {
      applyPatchToDir({
        patchFilePath: 'does-not-exist.patch',
        patchedDir: tempDir(),
      })
    }).toThrowError('Patch file not found')
  })
})
