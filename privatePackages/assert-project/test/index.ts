/// <reference path="../../../typings/index.d.ts"/>
import path from 'path'
import { assertProject } from '../src'

test('assertProject()', async () => {
  const project = assertProject(path.join(__dirname, '../../..'))

  await project.has('rimraf')
  await project.hasNot('sfdsff3g34')
  expect(typeof project.requireModule('rimraf')).toBe('function')
  await project.isExecutable('.bin/rimraf')
})

test('assertProject() store functions', async () => {
  const project = assertProject(path.join(__dirname, 'fixture/project'), 'registry.npmjs.org')

  expect(typeof await project.getStorePath()).toBe('string')
  await project.storeHas('is-positive', '3.1.0')
  expect(typeof await project.resolve('is-positive', '3.1.0')).toBe('string')
  await project.storeHasNot('is-positive', '3.100.0')
  expect(await project.readLockfile()).toBeTruthy()
  expect(await project.readCurrentLockfile()).toBeTruthy()
  expect(await project.readModulesManifest()).toBeTruthy()
})
