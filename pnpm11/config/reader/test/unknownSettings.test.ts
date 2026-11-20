import fs from 'node:fs'

import { expect, test } from '@jest/globals'
import camelcase from 'camelcase'

import { isKnownSettingKey, quoteAndAnnotateUnknown } from '../lib/unknownSettings.js'

test('quoteAndAnnotateUnknown() names the pnpm version that reads a setting this one does not', () => {
  const v12Settings = [
    'addMissingPeerTypes',
    'autoDedupe',
    'bootConfig',
    'cargo',
    'concurrencyGroups',
    'globalShims',
    'macosBackup',
    'nodePackageMap',
    'permissions',
    'pipelineBase',
    'pipelines',
    'publishWaitTimeout',
    'python',
    'remoteCache',
    'saveTypes',
    'skills',
    'storeIndexFormat',
    'tools',
  ]
  expect(v12Settings.filter(isKnownSettingKey)).toEqual([])
  expect(quoteAndAnnotateUnknown(v12Settings))
    .toBe(v12Settings.map((key) => `"${key}" (a pnpm v12 setting)`).join(', '))
})

test('quoteAndAnnotateUnknown() suggests a setting of this version for a typo', () => {
  expect(quoteAndAnnotateUnknown(['storeDur'])).toBe('"storeDur" (did you mean "storeDir"?)')
})

test('every WorkspaceSettings field is recognized or annotated as a pnpm v12 setting', () => {
  const settingsRs = fs.readFileSync(
    new URL('../../../../pnpm/crates/config/src/workspace_yaml/settings.rs', import.meta.url),
    'utf8'
  )
  const structBody = settingsRs.split('pub struct WorkspaceSettings {')[1].split('\n}')[0]
  const skippedFields = new Set(['auth', 'key_issues', 'includes'])
  const fields = structBody
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line.startsWith('pub '))
    .map((line) => line.slice('pub '.length).split(':')[0].trim())
    .filter((field) => !skippedFields.has(field))
    .map((field) => camelcase(field, { locale: 'en-US' }))

  expect(fields.length).toBeGreaterThan(0)
  for (const key of fields) {
    if (isKnownSettingKey(key)) continue
    expect(quoteAndAnnotateUnknown([key])).toBe(`"${key}" (a pnpm v12 setting)`)
  }
})
