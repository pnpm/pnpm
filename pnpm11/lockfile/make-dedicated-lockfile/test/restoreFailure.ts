import fs from 'node:fs'
import path from 'node:path'

import { expect, jest, test } from '@jest/globals'
import { fixtures } from '@pnpm/test-fixtures'

const { renameOverwrite: realRenameOverwrite } = await import('rename-overwrite')

const restoreError = Object.assign(new Error('EACCES: permission denied'), { code: 'EACCES' })
const renameOverwrite = jest.fn<typeof realRenameOverwrite>()
const pnpmExec = jest.fn(async () => {})

jest.unstable_mockModule('rename-overwrite', () => ({ renameOverwrite }))
jest.unstable_mockModule('@pnpm/exec', () => ({ pnpmExec }))

const { makeDedicatedLockfile } = await import('@pnpm/lockfile.make-dedicated-lockfile')

const f = fixtures(import.meta.dirname)

test('package.json is restored when node_modules cannot be moved back', async () => {
  const tmp = f.prepare('fixture')
  const projectDir = path.join(tmp, 'packages/published')
  const manifestPath = path.join(projectDir, 'package.json')
  const manifestBefore = { name: 'published', version: '1.0.0', publishConfig: { main: 'dist/index.js' } }
  fs.mkdirSync(path.join(projectDir, 'node_modules'), { recursive: true })
  fs.writeFileSync(manifestPath, JSON.stringify(manifestBefore))
  fs.writeFileSync(path.join(projectDir, 'node_modules/original-tree'), '')
  renameOverwrite
    .mockImplementationOnce(realRenameOverwrite)
    .mockRejectedValueOnce(restoreError)

  await expect(makeDedicatedLockfile(tmp, projectDir)).rejects.toBe(restoreError)

  expect(JSON.parse(fs.readFileSync(manifestPath, 'utf8'))).toStrictEqual(manifestBefore)
  expect(fs.existsSync(path.join(projectDir, '.tmp_node_modules/original-tree'))).toBe(true)
})

test('an install failure and a restore failure are reported together', async () => {
  const tmp = f.prepare('fixture')
  const projectDir = path.join(tmp, 'packages/published')
  fs.mkdirSync(path.join(projectDir, 'node_modules'), { recursive: true })
  fs.writeFileSync(path.join(projectDir, 'package.json'), JSON.stringify({ name: 'published', version: '1.0.0' }))
  const installError = new Error('install failed')
  pnpmExec.mockRejectedValueOnce(installError)
  renameOverwrite
    .mockImplementationOnce(realRenameOverwrite)
    .mockRejectedValueOnce(restoreError)

  const err = await makeDedicatedLockfile(tmp, projectDir).catch((err: unknown) => err)

  expect(err).toMatchObject({
    code: 'ERR_PNPM_MAKE_DEDICATED_LOCKFILE_FAILED',
    message: expect.stringContaining('install failed\nEACCES: permission denied'),
    hint: `The original node_modules is still in ${path.join(projectDir, '.tmp_node_modules')}.`,
    cause: {
      errors: [installError, restoreError],
      cause: installError,
    },
  })
})
