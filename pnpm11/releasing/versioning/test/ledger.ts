import fs from 'node:fs/promises'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { appendToLedger, readLedger } from '@pnpm/releasing.versioning'
import { temporaryDirectory } from 'tempy'

test('empty intent lists render as flow sequences and round-trip', async () => {
  const workspaceDir = temporaryDirectory()
  await appendToLedger(workspaceDir, { 'pacquet@12.0.0-alpha.13': { dir: 'pnpm/npm/pnpm', intents: [] } })
  const rendered = await fs.readFile(path.join(workspaceDir, '.changeset', 'ledger.yaml'), 'utf8')
  expect(rendered).toBe('pacquet@12.0.0-alpha.13:\n  dir: pnpm/npm/pnpm\n  intents: []\n')
  const ledger = await readLedger(workspaceDir)
  expect(ledger['pacquet@12.0.0-alpha.13']).toStrictEqual({ dir: 'pnpm/npm/pnpm', intents: [] })
})

test('null and missing intent lists parse as empty', async () => {
  const workspaceDir = temporaryDirectory()
  await fs.mkdir(path.join(workspaceDir, '.changeset'))
  await fs.writeFile(path.join(workspaceDir, '.changeset', 'ledger.yaml'), [
    'pacquet@12.0.0-alpha.13:\n  dir: pnpm/npm/pnpm\n  intents:\n',
    'pkg@1.0.0:\n',
    'other@2.0.0:\n  dir: packages/other\n',
  ].join(''))
  const ledger = await readLedger(workspaceDir)
  expect(ledger['pacquet@12.0.0-alpha.13']).toStrictEqual({ dir: 'pnpm/npm/pnpm', intents: [] })
  expect(ledger['pkg@1.0.0']).toStrictEqual([])
  expect(ledger['other@2.0.0']).toStrictEqual({ dir: 'packages/other', intents: [] })
})

test('an id-list entry with no ids renders as [] when the ledger is rewritten', async () => {
  const workspaceDir = temporaryDirectory()
  await fs.mkdir(path.join(workspaceDir, '.changeset'))
  await fs.writeFile(path.join(workspaceDir, '.changeset', 'ledger.yaml'), 'pkg@1.0.0: []\n')
  await appendToLedger(workspaceDir, { 'other@2.0.0': { dir: 'packages/other', intents: ['some-intent'] } })
  const rendered = await fs.readFile(path.join(workspaceDir, '.changeset', 'ledger.yaml'), 'utf8')
  expect(rendered).toBe('other@2.0.0:\n  dir: packages/other\n  intents:\n    - some-intent\npkg@1.0.0: []\n')
  const ledger = await readLedger(workspaceDir)
  expect(ledger['pkg@1.0.0']).toStrictEqual([])
})
