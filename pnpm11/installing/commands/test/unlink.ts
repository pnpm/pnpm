import fs from 'node:fs'

import { expect, test } from '@jest/globals'
import { link, unlink } from '@pnpm/installing.commands'
import { prepare } from '@pnpm/prepare'
import { loadJsonFileSync } from 'load-json-file'
import { writePackageSync } from 'write-package'

import { DEFAULT_OPTS } from './utils/index.js'

function prepareLinkTargets () {
  const project = prepare({ name: 'project', version: '1.0.0' })
  process.chdir('..')
  writePackageSync('linked-foo', { name: 'linked-foo', version: '1.0.0' })
  writePackageSync('linked-bar', { name: 'linked-bar', version: '1.0.0' })
  process.chdir('project')
  return project
}

function commandOpts (overrides?: Record<string, string>) {
  return {
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    overrides,
    rootProjectManifest: loadJsonFileSync<{ name: string }>('package.json'),
    rootProjectManifestDir: process.cwd(),
  }
}

test('unlink <pkg> removes the dependency that link added to package.json', async () => {
  const project = prepareLinkTargets()

  await link.handler(commandOpts(), ['../linked-foo', '../linked-bar'])
  project.has('linked-foo')

  await unlink.handler(commandOpts({ 'linked-foo': 'link:../linked-foo', 'linked-bar': 'link:../linked-bar' }), ['linked-foo'])

  expect(loadJsonFileSync('package.json')).toStrictEqual({
    name: 'project',
    version: '1.0.0',
    dependencies: { 'linked-bar': 'link:../linked-bar' },
  })
  expect(fs.readFileSync('pnpm-workspace.yaml', 'utf8')).not.toContain('linked-foo')
  project.hasNot('linked-foo')
  project.has('linked-bar')
})

test('unlink without arguments reverts every link made by link', async () => {
  const project = prepareLinkTargets()

  await link.handler(commandOpts(), ['../linked-foo', '../linked-bar'])

  await unlink.handler(commandOpts({ 'linked-foo': 'link:../linked-foo', 'linked-bar': 'link:../linked-bar' }), [])

  expect(loadJsonFileSync('package.json')).toStrictEqual({ name: 'project', version: '1.0.0' })
  expect(fs.existsSync('pnpm-workspace.yaml')).toBe(false)
  project.hasNot('linked-foo')
  project.hasNot('linked-bar')
})

test('unlink keeps a link: dependency that points to another directory than the override', async () => {
  prepareLinkTargets()
  writePackageSync('.', {
    name: 'project',
    version: '1.0.0',
    dependencies: { 'linked-foo': 'link:../linked-bar' },
  })

  await unlink.handler(commandOpts({ 'linked-foo': 'link:../linked-foo' }), ['linked-foo'])

  expect(loadJsonFileSync('package.json')).toStrictEqual({
    name: 'project',
    version: '1.0.0',
    dependencies: { 'linked-foo': 'link:../linked-bar' },
  })
})
