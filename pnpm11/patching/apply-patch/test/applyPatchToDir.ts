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
const { executeEffects } = await import('../src/executeEffects.js')
const { parsePatchFile } = await import('../src/parsePatch.js')

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
    const effects = parsePatchFile(`diff --git a/index.js b/Index.js
similarity index 100%
rename from index.js
rename to Index.js
`)
    expect(() => {
      executeEffects(effects, { root: patchedDir, dryRun: false })
    }).toThrow('Trying to move a file onto one that already exists: Index.js')
    expect(fs.lstatSync(path.join(patchedDir, 'index.js')).isSymbolicLink()).toBe(true)
    expect(fs.readFileSync(path.join(patchedDir, 'Index.js'), 'utf8')).toBe('module.exports = 1\n')
  })
  // Creating symlinks on Windows needs a privilege CI does not have.
  const itWithSymlinks = process.platform === 'win32' ? it.skip : it
  itWithSymlinks('should rename a dangling symlink', () => {
    const patchedDir = tempDir()
    fs.symlinkSync('missing-target', path.join(patchedDir, 'old-link'))
    const patchFilePath = path.join(patchedDir, '..', 'rename-dangling.patch')
    fs.writeFileSync(patchFilePath, `diff --git a/old-link b/new-link
similarity index 100%
rename from old-link
rename to new-link
`)
    expect(applyPatchToDir({ patchFilePath, patchedDir })).toBe(true)
    expect(applyPatchToDir({ patchFilePath, patchedDir })).toBe(true)
    expect(fs.readlinkSync(path.join(patchedDir, 'new-link'))).toBe('missing-target')
    expect(fs.existsSync(path.join(patchedDir, 'old-link'))).toBe(false)
  })
  it('should take a patch that renames a file and recreates its old path as already applied', () => {
    const patchedDir = tempDir()
    fs.writeFileSync(path.join(patchedDir, 'a.txt'), 'old\n')
    const patchFilePath = path.join(patchedDir, '..', 'rename-recreate.patch')
    fs.writeFileSync(patchFilePath, `diff --git a/a.txt b/b.txt
similarity index 100%
rename from a.txt
rename to b.txt
diff --git a/a.txt b/a.txt
new file mode 100644
--- /dev/null
+++ b/a.txt
@@ -0,0 +1 @@
+new
`)
    expect(applyPatchToDir({ patchFilePath, patchedDir })).toBe(true)
    expect(applyPatchToDir({ patchFilePath, patchedDir })).toBe(true)
    expect(fs.readFileSync(path.join(patchedDir, 'a.txt'), 'utf8')).toBe('new\n')
    expect(fs.readFileSync(path.join(patchedDir, 'b.txt'), 'utf8')).toBe('old\n')
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
  // Patches written by hand or by older `pnpm patch-commit` versions can lack
  // the final "No newline at end of file" marker of such an append.
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
  it('should delete a file whose name contains " b/" without touching another file', () => {
    const patchedDir = tempDir()
    fs.writeFileSync(path.join(patchedDir, 'foo'), 'keep\n')
    fs.mkdirSync(path.join(patchedDir, 'foo b'))
    fs.writeFileSync(path.join(patchedDir, 'foo b/bar.js'), 'gone\n')
    const patchFilePath = path.join(patchedDir, '..', 'delete.patch')
    fs.writeFileSync(patchFilePath, `diff --git a/foo b/bar.js b/foo b/bar.js
deleted file mode 100644
index 1111111111111111111111111111111111111111..0000000000000000000000000000000000000000
`)
    applyPatchToDir({ patchFilePath, patchedDir })
    expect(fs.existsSync(path.join(patchedDir, 'foo b/bar.js'))).toBe(false)
    expect(fs.readFileSync(path.join(patchedDir, 'foo'), 'utf8')).toBe('keep\n')
  })
  it('should insert the lines of a zero-context hunk after the line its header names', () => {
    const patchedDir = tempDir()
    fs.writeFileSync(path.join(patchedDir, 'file.txt'), 'one\ntwo\n')
    const patchFilePath = path.join(patchedDir, '..', 'insert.patch')
    fs.writeFileSync(patchFilePath, `diff --git a/file.txt b/file.txt
--- a/file.txt
+++ b/file.txt
@@ -1,0 +2 @@
+added
`)
    applyPatchToDir({ patchFilePath, patchedDir })
    expect(fs.readFileSync(path.join(patchedDir, 'file.txt'), 'utf8')).toBe('one\nadded\ntwo\n')
  })
  it('should insert the lines of a zero-context hunk at the start of the file', () => {
    const patchedDir = tempDir()
    fs.writeFileSync(path.join(patchedDir, 'file.txt'), 'one\ntwo\n')
    const patchFilePath = path.join(patchedDir, '..', 'insert.patch')
    fs.writeFileSync(patchFilePath, `diff --git a/file.txt b/file.txt
--- a/file.txt
+++ b/file.txt
@@ -0,0 +1 @@
+added
`)
    applyPatchToDir({ patchFilePath, patchedDir })
    expect(fs.readFileSync(path.join(patchedDir, 'file.txt'), 'utf8')).toBe('added\none\ntwo\n')
  })
  it('should not overwrite an existing file with a file the patch creates', () => {
    const patchedDir = tempDir()
    fs.writeFileSync(path.join(patchedDir, 'file.txt'), 'existing\n')
    const patchFilePath = path.join(patchedDir, '..', 'create.patch')
    fs.writeFileSync(patchFilePath, `diff --git a/file.txt b/file.txt
new file mode 100644
--- /dev/null
+++ b/file.txt
@@ -0,0 +1 @@
+created
`)
    expect(() => applyPatchToDir({ patchFilePath, patchedDir })).toThrow(`Could not apply patch ${patchFilePath} to ${patchedDir}`)
    expect(fs.readFileSync(path.join(patchedDir, 'file.txt'), 'utf8')).toBe('existing\n')
  })
  it('should take a created file that already holds the created content as applied', () => {
    const patchedDir = tempDir()
    fs.writeFileSync(path.join(patchedDir, 'file.txt'), 'created\n')
    const patchFilePath = path.join(patchedDir, '..', 'create.patch')
    fs.writeFileSync(patchFilePath, `diff --git a/file.txt b/file.txt
new file mode 100644
--- /dev/null
+++ b/file.txt
@@ -0,0 +1 @@
+created
`)
    expect(applyPatchToDir({ patchFilePath, patchedDir })).toBe(true)
    expect(fs.readFileSync(path.join(patchedDir, 'file.txt'), 'utf8')).toBe('created\n')
  })
  it('should not take an empty file the patch creates as applied over a nonempty file', () => {
    const patchedDir = tempDir()
    fs.writeFileSync(path.join(patchedDir, 'file.txt'), 'existing\n')
    const patchFilePath = path.join(patchedDir, '..', 'create-empty.patch')
    fs.writeFileSync(patchFilePath, `diff --git a/file.txt b/file.txt
new file mode 100644
index 0000000000000000000000000000000000000000..e69de29bb2d1d6434b8b29ae775ad8c2e48c5391
`)
    expect(() => applyPatchToDir({ patchFilePath, patchedDir })).toThrow(`Could not apply patch ${patchFilePath} to ${patchedDir}`)
    expect(fs.readFileSync(path.join(patchedDir, 'file.txt'), 'utf8')).toBe('existing\n')
  })
  it('should not take a file whose bytes differ from the created content as applied', () => {
    const patchedDir = tempDir()
    fs.writeFileSync(path.join(patchedDir, 'file.txt'), Buffer.from([0xff, 0x0a]))
    const patchFilePath = path.join(patchedDir, '..', 'create.patch')
    fs.writeFileSync(patchFilePath, `diff --git a/file.txt b/file.txt
new file mode 100644
--- /dev/null
+++ b/file.txt
@@ -0,0 +1 @@
+\uFFFD
`)
    expect(() => applyPatchToDir({ patchFilePath, patchedDir })).toThrow(`Could not apply patch ${patchFilePath} to ${patchedDir}`)
    expect(fs.readFileSync(path.join(patchedDir, 'file.txt'))).toStrictEqual(Buffer.from([0xff, 0x0a]))
  })
  itWithExecutableBits('should give an already created file the mode the patch creates it with', () => {
    const patchedDir = tempDir()
    const scriptPath = path.join(patchedDir, 'run.sh')
    fs.writeFileSync(scriptPath, 'echo 1\n', { mode: 0o644 })
    const patchFilePath = path.join(patchedDir, '..', 'create-executable.patch')
    fs.writeFileSync(patchFilePath, `diff --git a/run.sh b/run.sh
new file mode 100755
--- /dev/null
+++ b/run.sh
@@ -0,0 +1 @@
+echo 1
`)
    expect(applyPatchToDir({ patchFilePath, patchedDir })).toBe(true)
    expect(fs.statSync(scriptPath).mode & 0o777).toBe(0o755)
  })
})
