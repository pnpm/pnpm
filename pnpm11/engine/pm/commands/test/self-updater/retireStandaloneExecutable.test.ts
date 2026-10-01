import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { tempDir } from '@pnpm/prepare'

import { linkReplacingRetiredExecutable, retireStandaloneExecutable } from '../../src/self-updater/retireStandaloneExecutable.js'

test('linkReplacingRetiredExecutable restores pnpm.exe when linking fails', async () => {
  const dir = tempDir(false)
  fs.writeFileSync(path.join(dir, 'pnpm.exe'), 'old standalone pnpm')
  const retired = await retireStandaloneExecutable(dir)
  expect(fs.existsSync(path.join(dir, 'pnpm.exe'))).toBe(false)

  const linkError = new Error('link failed')
  await expect(linkReplacingRetiredExecutable(retired, () => Promise.reject(linkError))).rejects.toBe(linkError)

  expect(fs.readFileSync(path.join(dir, 'pnpm.exe'), 'utf8')).toBe('old standalone pnpm')
  expect(fs.readdirSync(dir)).toStrictEqual(['pnpm.exe'])
})

test('linkReplacingRetiredExecutable reports a pnpm.exe it cannot restore', async () => {
  const dir = tempDir(false)
  fs.writeFileSync(path.join(dir, 'pnpm.exe'), 'old standalone pnpm')
  const retired = await retireStandaloneExecutable(dir)
  fs.rmSync(retired!.retired)

  await expect(linkReplacingRetiredExecutable(retired, () => Promise.reject(new Error('link failed')))).rejects.toMatchObject({
    code: 'ERR_PNPM_SELF_UPDATE_RESTORE_FAILED',
  })
  expect(fs.existsSync(path.join(dir, 'pnpm.exe'))).toBe(false)
})
