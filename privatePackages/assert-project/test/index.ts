/// <reference path="../../../typings/index.d.ts"/>
import assertProject from '../src'
import path = require('path')

test('assertProject()', async () => {
  const project = assertProject(path.join(__dirname, '../../..'))

  await project.has('tape')
  await project.hasNot('sfdsff3g34')
  expect(typeof project.requireModule('tape')).toBe('function')
  await project.isExecutable('.bin/tape')
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
