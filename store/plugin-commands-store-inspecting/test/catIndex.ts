/// <reference path="../../../__typings__/index.d.ts" />
import path from 'path'

import { prepare } from '@pnpm/prepare'
import { getConfig } from '@pnpm/config'
import { catIndex } from '@pnpm/plugin-commands-store-inspecting'
import { type PnpmError } from '@pnpm/error'

import execa from 'execa'

const pnpmBin = path.join(__dirname, '../../../pnpm/bin/pnpm.cjs')

// cat-index
test('print cat index file content', async () => {
  prepare({
    dependencies: {
      bytes: '3.1.2',
    },
  })

  await execa('node', [pnpmBin, 'install'])

  {
    const { config } = await getConfig({
      cliOptions: {},
      packageManager: {
        name: 'pnpm',
        version: '8.12.1',
      },
    })
    const output = await catIndex.handler(config as catIndex.CatIndexCommandOptions, ['bytes@3.1.2'])

    expect(output).toBeTruthy()
    expect(typeof JSON.parse(output).files['package.json'].checkedAt).toBeTruthy()
  }
})

test('prints index file error', async () => {
  let err!: PnpmError
  try {
    const { config } = await getConfig({
      cliOptions: {},
      packageManager: {
        name: 'pnpm',
        version: '8.12.1',
      },
    })
    await catIndex.handler(config as catIndex.CatIndexCommandOptions, ['bytes@3.1.1'])
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }

  expect(err.code).toBe('ERR_PNPM_INVALID_PACKAGE')
  expect(err.message).toBe('No corresponding index file found. You can use pnpm list to see if the package is installed.')
})