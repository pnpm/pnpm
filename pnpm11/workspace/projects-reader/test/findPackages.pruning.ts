import fs from 'node:fs'
import path from 'node:path'

import { expect, jest, test } from '@jest/globals'
import { temporaryDirectory } from 'tempy'

const readdir = jest.fn(fs.readdir)
const readdirSync = jest.fn(fs.readdirSync)
jest.unstable_mockModule('fs', () => ({ ...fs, default: fs, readdir, readdirSync }))

const { findPackages, findPackagesSync } = await import('@pnpm/workspace.projects-reader')

test.each([false, true])('does not read excluded subtrees (sync: %s)', async (sync) => {
  const root = temporaryDirectory()
  for (const dir of ['keep', 'generated/child']) {
    fs.mkdirSync(path.join(root, dir), { recursive: true })
    fs.writeFileSync(path.join(root, dir, 'package.json'), JSON.stringify({ name: dir }))
  }
  const readDirectory = sync ? readdirSync : readdir
  readDirectory.mockClear()
  const options = { patterns: ['**', '!generated/**'] }
  const projects = sync ? findPackagesSync(root, options) : await findPackages(root, options)
  expect(projects.map(({ manifest }) => manifest.name)).toEqual(['keep'])
  const directories = readDirectory.mock.calls.map(([directory]) => path.resolve(String(directory)))
  expect(directories).toContain(root)
  expect(directories).toContain(path.join(root, 'keep'))
  expect(directories).not.toContain(path.join(root, 'generated'))
})
