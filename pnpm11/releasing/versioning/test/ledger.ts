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

test('null entries and null intents parse as empty lists', async () => {
  // The shape ledgers written before empty lists rendered as `[]` contain.
  const workspaceDir = temporaryDirectory()
  await fs.mkdir(path.join(workspaceDir, '.changeset'))
  await fs.writeFile(path.join(workspaceDir, '.changeset', 'ledger.yaml'), 'pacquet@12.0.0-alpha.13:\n  dir: pnpm/npm/pnpm\n  intents:\npkg@1.0.0:\n')
  const ledger = await readLedger(workspaceDir)
  expect(ledger['pacquet@12.0.0-alpha.13']).toStrictEqual({ dir: 'pnpm/npm/pnpm', intents: [] })
  expect(ledger['pkg@1.0.0']).toStrictEqual([])
})
