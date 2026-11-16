import fs from 'node:fs'
import path from 'node:path'

import { beforeEach, describe, expect, it, jest } from '@jest/globals'
import { tempDir } from '@pnpm/prepare'
import { fixtures } from '@pnpm/test-fixtures'

const testFixtures = fixtures(import.meta.dirname)

const originalModule = await import('@pnpm/logger')
jest.unstable_mockModule('@pnpm/logger', () => {
  return {
    ...originalModule,
    globalWarn: jest.fn(),
  }
})

const { globalWarn } = await import('@pnpm/logger')
const { applyPatchToDir } = await import('@pnpm/patching.apply-patch')

beforeEach(() => {
  jest.mocked(globalWarn).mockClear()
})

function isCaseSensitive (dir: string): boolean {
  fs.writeFileSync(path.join(dir, 'probe'), '')
  return !fs.existsSync(path.join(dir, 'PROBE'))
}

function prepareDirToPatch () {
  const dir = tempDir()
  testFixtures.copy('patch-target.txt', path.join(dir, 'patch-target.txt'))
  return dir
}

describe('applyPatchToDir()', () => {
  it('should succeed when patch is applicable', () => {
    const patchFilePath = testFixtures.find('applicable.patch')
    const successfullyPatched = testFixtures.find('successfully-patched.txt')
    const patchedDir = prepareDirToPatch()
    expect(
      applyPatchToDir({
        patchFilePath,
        patchedDir,
      })
    ).toBe(true)
    const patchTarget = path.join(patchedDir, 'patch-target.txt')
    expect(fs.readFileSync(patchTarget, 'utf-8')).toBe(fs.readFileSync(successfullyPatched, 'utf-8'))
  })
  it('should fail when patch fails to apply', () => {
    const patchFilePath = testFixtures.find('non-applicable.patch')
    const patchedDir = prepareDirToPatch()
    expect(() => {
      applyPatchToDir({
        patchFilePath,
        patchedDir,
      })
    }).toThrow(`Could not apply patch ${patchFilePath} to ${patchedDir}`)
    expect(fs.readFileSync(path.join(patchedDir, 'patch-target.txt'), 'utf-8')).toBe(fs.readFileSync(testFixtures.find('patch-target.txt'), 'utf-8'))
  })
  it('should fail on invalid patch', () => {
    const patchFilePath = testFixtures.find('invalid.patch')
    expect(() => {
      applyPatchToDir({
        patchFilePath,
        patchedDir: tempDir(),
      })
    }).toThrow(`Applying patch "${patchFilePath}" failed: hunk header integrity check failed`)
  })
  it('should fail if the patch file is not found', () => {
    expect(() => {
      applyPatchToDir({
        patchFilePath: 'does-not-exist.patch',
        patchedDir: tempDir(),
      })
    }).toThrow('Patch file not found')
  })
  it('should reject a patch whose paths escape the patched directory', () => {
    const patchFilePath = testFixtures.find('path-traversal.patch')
    const patchedDir = tempDir()
    const sentinel = path.join('/tmp', 'pnpm-patch-traversal-pwned')
    try {
      fs.unlinkSync(sentinel)
    } catch {}
    expect(() => {
      applyPatchToDir({
        patchFilePath,
        patchedDir,
      })
    }).toThrow(/patch path escapes target dir/)
    expect(fs.existsSync(sentinel)).toBe(false)
  })
  it.each([
    ['a Windows-separated ".." path', (outsideDir: string) => path.relative(path.join(outsideDir, 'pkg'), path.join(outsideDir, 'escaped.txt')).replaceAll('/', '\\')],
    ['an absolute path', (outsideDir: string) => path.join(outsideDir, 'escaped.txt').replaceAll('\\', '/')],
  ])('should reject a patch with %s', (_name, escapingPath) => {
    const outsideDir = tempDir(false)
    const patchedDir = path.join(outsideDir, 'pkg')
    fs.mkdirSync(patchedDir)
    const target = escapingPath(outsideDir)
    const patchFilePath = path.join(outsideDir, 'escape.patch')
    fs.writeFileSync(patchFilePath, `diff --git a/${target} b/${target}
new file mode 100644
--- /dev/null
+++ b/${target}
@@ -0,0 +1 @@
+escaped
`)
    expect(() => applyPatchToDir({ patchFilePath, patchedDir })).toThrow(/patch path escapes target dir/)
    expect(fs.existsSync(path.join(outsideDir, 'escaped.txt'))).toBe(false)
  })
  // On a case-insensitive file system the renamed-to file already exists:
  // it is the file being renamed.
  it('should rename a file to a name that differs only in case', () => {
    const patchedDir = tempDir()
    fs.writeFileSync(path.join(patchedDir, 'index.js'), 'module.exports = 1\n')
    const patchFilePath = path.join(patchedDir, '..', 'rename-case.patch')
    fs.writeFileSync(patchFilePath, `diff --git a/index.js b/Index.js
similarity index 100%
rename from index.js
rename to Index.js
`)
    expect(applyPatchToDir({ patchFilePath, patchedDir })).toBe(true)
    expect(fs.readdirSync(patchedDir)).toStrictEqual(['Index.js'])
    expect(fs.readFileSync(path.join(patchedDir, 'Index.js'), 'utf8')).toBe('module.exports = 1\n')
  })
  // index.js and Index.js can only coexist on a case-sensitive file system,
  // which excludes the defaults on macOS and Windows.
  const itWithCaseVariants = isCaseSensitive(tempDir(false)) ? it : it.skip
  itWithCaseVariants('should not rename a symlink onto the file it points to', () => {
    const patchedDir = tempDir()
    fs.writeFileSync(path.join(patchedDir, 'Index.js'), 'module.exports = 1\n')
    fs.symlinkSync('Index.js', path.join(patchedDir, 'index.js'))
    const patchFilePath = path.join(patchedDir, '..', 'rename-symlink.patch')
    fs.writeFileSync(patchFilePath, `diff --git a/index.js b/Index.js
similarity index 100%
rename from index.js
rename to Index.js
`)
    expect(() => applyPatchToDir({ patchFilePath, patchedDir })).toThrow(`Could not apply patch ${patchFilePath} to ${patchedDir}`)
    expect(fs.lstatSync(path.join(patchedDir, 'index.js')).isSymbolicLink()).toBe(true)
    expect(fs.lstatSync(path.join(patchedDir, 'Index.js')).isFile()).toBe(true)
    expect(fs.readFileSync(path.join(patchedDir, 'Index.js'), 'utf8')).toBe('module.exports = 1\n')
  })
  // `git diff --ignore-cr-at-eol` writes lines appended to a file without a
  // final newline as context followed by the insertion.
  it('should append after a last line that has no final newline but is matched as context', () => {
    const patchedDir = tempDir()
    fs.writeFileSync(path.join(patchedDir, 'index.js'), 'function f() {\n  return 1;\n}\nmodule.exports = f;')
    const patchFilePath = path.join(patchedDir, 'append.patch')
    fs.writeFileSync(patchFilePath, `diff --git a/index.js b/index.js
--- a/index.js
+++ b/index.js
@@ -2,3 +2,6 @@ function f() {
   return 1;
 }
 module.exports = f;
+function g() {
+  return 2;
+}
\\ No newline at end of file
`)
    applyPatchToDir({ patchFilePath, patchedDir })
    expect(fs.readFileSync(path.join(patchedDir, 'index.js'), 'utf-8'))
      .toBe('function f() {\n  return 1;\n}\nmodule.exports = f;\nfunction g() {\n  return 2;\n}')
  })
  // `pnpm patch-commit` strips the patch's final "No newline at end of file"
  // marker, so an append that keeps the file without a final newline arrives unmarked.
  it('should keep a file without a final newline unterminated after an unmarked append', () => {
    const patchedDir = tempDir()
    fs.writeFileSync(path.join(patchedDir, 'file.txt'), 'one\ntwo')
    const patchFilePath = path.join(patchedDir, 'append.patch')
    fs.writeFileSync(patchFilePath, `diff --git a/file.txt b/file.txt
--- a/file.txt
+++ b/file.txt
@@ -1,2 +1,3 @@
 one
 two
+three
`)
    applyPatchToDir({ patchFilePath, patchedDir })
    expect(fs.readFileSync(path.join(patchedDir, 'file.txt'), 'utf-8')).toBe('one\ntwo\nthree')
  })
  // pnpm takes a patch as already applied when its reverse applies cleanly.
  // Checking that must not change the file it checks.
  const itWithExecutableBits = process.platform === 'win32' ? it.skip : it
  itWithExecutableBits('should keep the mode an already applied patch set', () => {
    const patchedDir = tempDir()
    const scriptPath = path.join(patchedDir, 'run.sh')
    fs.writeFileSync(scriptPath, 'echo 2\n')
    fs.chmodSync(scriptPath, 0o755)
    const patchFilePath = path.join(patchedDir, 'mode.patch')
    fs.writeFileSync(patchFilePath, `diff --git a/run.sh b/run.sh
old mode 100644
new mode 100755
--- a/run.sh
+++ b/run.sh
@@ -1 +1 @@
-echo 1
+echo 2
`)
    expect(applyPatchToDir({ patchFilePath, patchedDir })).toBe(true)
    expect(fs.statSync(scriptPath).mode & 0o777).toBe(0o755)
    expect(fs.readFileSync(scriptPath, 'utf8')).toBe('echo 2\n')
  })
})
