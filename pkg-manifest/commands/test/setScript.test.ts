import fs from 'node:fs'
import path from 'node:path'

import { beforeEach, describe, expect, test } from '@jest/globals'
import { setScript } from '@pnpm/pkg-manifest.commands'
import { tempDir } from '@pnpm/prepare'

describe('set-script command', () => {
  let tmpDir: string

  beforeEach(() => {
    tmpDir = tempDir()
    fs.writeFileSync(
      path.join(tmpDir, 'package.json'),
      JSON.stringify({ name: 'test-package', version: '1.0.0' }, null, 2)
    )
  })

  test('exposes the ss alias', () => {
    expect(setScript.commandNames).toEqual(['set-script', 'ss'])
  })

  test('adds a script when none exist', async () => {
    await setScript.handler({ dir: tmpDir }, ['build', 'tsc -b'])

    const written = JSON.parse(fs.readFileSync(path.join(tmpDir, 'package.json'), 'utf8'))
    expect(written.scripts).toEqual({ build: 'tsc -b' })
  })

  test('overwrites an existing script', async () => {
    fs.writeFileSync(
      path.join(tmpDir, 'package.json'),
      JSON.stringify({ name: 'test-package', scripts: { build: 'old' } }, null, 2)
    )

    await setScript.handler({ dir: tmpDir }, ['build', 'tsc -b'])

    const written = JSON.parse(fs.readFileSync(path.join(tmpDir, 'package.json'), 'utf8'))
    expect(written.scripts.build).toBe('tsc -b')
  })

  test('joins remaining params into the command', async () => {
    await setScript.handler({ dir: tmpDir }, ['lint', 'eslint', '--fix', 'src'])

    const written = JSON.parse(fs.readFileSync(path.join(tmpDir, 'package.json'), 'utf8'))
    expect(written.scripts.lint).toBe('eslint --fix src')
  })

  test('accepts script names that contain dots, hyphens, and quotes', async () => {
    await setScript.handler({ dir: tmpDir }, ['my-build', 'tsc -b'])
    await setScript.handler({ dir: tmpDir }, ['pre.publish', 'echo'])
    await setScript.handler({ dir: tmpDir }, ['weird"name', 'echo weird'])

    const written = JSON.parse(fs.readFileSync(path.join(tmpDir, 'package.json'), 'utf8'))
    expect(written.scripts).toEqual({
      'my-build': 'tsc -b',
      'pre.publish': 'echo',
      'weird"name': 'echo weird',
    })
  })

  test('accepts script names containing an equals sign', async () => {
    await setScript.handler({ dir: tmpDir }, ['with=eq', 'echo with=eq'])

    const written = JSON.parse(fs.readFileSync(path.join(tmpDir, 'package.json'), 'utf8'))
    expect(written.scripts['with=eq']).toBe('echo with=eq')
  })

  test('throws when arguments are missing', async () => {
    await expect(setScript.handler({ dir: tmpDir }, ['build']))
      .rejects.toThrow('Missing script name or command')
  })

  test('rejects unsafe script names', async () => {
    await expect(setScript.handler({ dir: tmpDir }, ['__proto__', 'echo']))
      .rejects.toThrow()
  })
})
