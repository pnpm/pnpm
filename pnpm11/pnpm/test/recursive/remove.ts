import fs from 'node:fs'

import { expect, test } from '@jest/globals'
import { preparePackages } from '@pnpm/prepare'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpm } from '../utils/index.js'

test.each([
  ['missing'],
  ['is-positive', 'missing'],
  ['--filter=project-2', 'is-positive'],
  ['--save-dev', 'is-positive'],
  ['--save-optional', 'is-positive'],
  ['--save-prod', 'is-negative'],
])('recursive remove validates the selected dependencies before writing: %j', async (...args) => {
  prepareRemovalWorkspace()
  const files = ['project-1/package.json', 'project-2/package.json', 'pnpm-workspace.yaml']
  const before = files.map(file => fs.readFileSync(file, 'utf8'))

  await expect(execPnpm(['remove', '-r', '--lockfile-only', ...args]))
    .rejects.toThrow('ERR_PNPM_CANNOT_REMOVE_MISSING_DEPS')

  expect(files.map(file => fs.readFileSync(file, 'utf8'))).toEqual(before)
  expect(fs.existsSync('pnpm-lock.yaml')).toBe(false)
})

test('recursive remove accepts dependencies spread across selected projects', async () => {
  prepareRemovalWorkspace()

  await execPnpm(['remove', '-r', '--lockfile-only', 'is-positive', 'is-negative'])

  for (const name of ['project-1', 'project-2']) {
    const manifest = JSON.parse(fs.readFileSync(`${name}/package.json`, 'utf8'))
    expect(manifest.dependencies ?? {}).toEqual({})
    expect(manifest.devDependencies ?? {}).toEqual({})
  }
  expect(fs.existsSync('pnpm-lock.yaml')).toBe(true)
})

function prepareRemovalWorkspace (): void {
  preparePackages([
    { name: 'project-1', version: '1.0.0', dependencies: { 'is-positive': '1.0.0' } },
    { name: 'project-2', version: '1.0.0', devDependencies: { 'is-negative': '1.0.0' } },
  ])
  writeYamlFileSync('pnpm-workspace.yaml', { packages: ['project-*'] })
}
