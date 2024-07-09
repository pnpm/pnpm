/// <reference path="../../../__typings__/index.d.ts"/>
import path from 'path'
import { assertProject } from '../src'

test('assertProject()', async () => {
  const project = assertProject(path.join(__dirname, '../../..'))

  project.has('rimraf')
  project.hasNot('sfdsff3g34') // cspell:disable-line
  expect(typeof project.requireModule('rimraf')).toBe('function')
  project.isExecutable('.bin/rimraf')
})

test('assertProject() store functions', async () => {
  const project = assertProject(path.join(__dirname, 'fixture/project'), 'registry.npmjs.org')

  expect(typeof project.getStorePath()).toBe('string')
  project.storeHas('is-positive', '3.1.0')
  expect(typeof project.resolve('is-positive', '3.1.0')).toBe('string')
  project.storeHasNot('is-positive', '3.100.0')
  expect(project.readLockfile()).toBeTruthy()
  expect(project.readCurrentLockfile()).toBeTruthy()
  expect(project.readModulesManifest()).toBeTruthy()
})
