import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterEach, expect, jest, test } from '@jest/globals'

import { safeIsInnerLink } from '../src/safeIsInnerLink.js'

const platform = Object.getOwnPropertyDescriptor(process, 'platform')!
const projectDirs: string[] = []

afterEach(() => {
  jest.restoreAllMocks()
  Object.defineProperty(process, 'platform', platform)
  for (const projectDir of projectDirs.splice(0)) {
    fs.rmSync(projectDir, { recursive: true, force: true })
  }
})

function createAlienModule (): { modulesDir: string, opts: Parameters<typeof safeIsInnerLink>[2] } {
  const projectDir = fs.mkdtempSync(path.join(os.tmpdir(), 'safe-is-inner-link-'))
  projectDirs.push(projectDir)
  const modulesDir = path.join(projectDir, 'node_modules')
  fs.mkdirSync(path.join(modulesDir, '@types/node'), { recursive: true })
  fs.writeFileSync(path.join(modulesDir, '@types/node/index.d.ts'), '')
  const virtualStoreDir = path.join(modulesDir, '.pnpm')
  return {
    modulesDir,
    opts: { hideAlienModules: true, projectDir, virtualStoreDir, globalVirtualStoreDir: virtualStoreDir },
  }
}

test('a directory installed by another package manager is moved to node_modules/.ignored', async () => {
  const { modulesDir, opts } = createAlienModule()
  fs.mkdirSync(path.join(modulesDir, '.ignored/@types/node/stale'), { recursive: true })

  expect(await safeIsInnerLink(modulesDir, '@types/node', opts)).toBe(true)

  expect(fs.existsSync(path.join(modulesDir, '@types/node'))).toBe(false)
  expect(fs.readdirSync(path.join(modulesDir, '.ignored/@types/node'))).toStrictEqual(['index.d.ts'])
})

test('a directory locked by another process on Windows fails within a second and names the directory', async () => {
  const { modulesDir, opts } = createAlienModule()
  Object.defineProperty(process, 'platform', { value: 'win32' })
  let elapsed = 0
  jest.spyOn(Date, 'now').mockImplementation(() => elapsed)
  jest.spyOn(Atomics, 'wait').mockImplementation((_array, _index, _value, delay) => {
    elapsed += delay!
    return 'timed-out'
  })
  const locked = Object.assign(new Error('EPERM: operation not permitted, rename'), { code: 'EPERM' })
  jest.spyOn(fs, 'renameSync').mockImplementation(() => {
    throw locked
  })

  const promise = safeIsInnerLink(modulesDir, '@types/node', opts)

  await expect(promise).rejects.toMatchObject({
    code: 'ERR_PNPM_MODULES_DIR_IN_USE',
    message: expect.stringContaining(`"${path.join(modulesDir, '@types/node')}"`),
    hint: expect.stringContaining('in use by another process'),
    cause: locked,
  })
  expect(elapsed).toBe(1_000)
})

test('a locked leftover in node_modules/.ignored on Windows is named in the error', async () => {
  const { modulesDir, opts } = createAlienModule()
  Object.defineProperty(process, 'platform', { value: 'win32' })
  const locked = Object.assign(new Error('EBUSY: resource busy or locked, rmdir'), { code: 'EBUSY' })
  jest.spyOn(fs.promises, 'rm').mockRejectedValue(locked)

  await expect(safeIsInnerLink(modulesDir, '@types/node', opts)).rejects.toMatchObject({
    code: 'ERR_PNPM_MODULES_DIR_IN_USE',
    message: `Could not remove "${path.join(modulesDir, '.ignored', '@types/node')}" (EBUSY)`,
    cause: locked,
  })
  expect(fs.existsSync(path.join(modulesDir, '@types/node/index.d.ts'))).toBe(true)
})
