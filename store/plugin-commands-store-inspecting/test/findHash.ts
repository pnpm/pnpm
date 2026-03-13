/// <reference path="../../../__typings__/index.d.ts" />
import path from 'node:path'

import { getConfig } from '@pnpm/config'
import type { PnpmError } from '@pnpm/error'
import { findHash } from '@pnpm/plugin-commands-store-inspecting'
import { prepare } from '@pnpm/prepare'
import { safeExeca as execa } from 'execa'
import { temporaryDirectory } from 'tempy'

const pnpmBin = path.join(import.meta.dirname, '../../../pnpm/bin/pnpm.mjs')

test('print index file path with hash', async () => {
  const { PACKAGE_INFO_CLR } = findHash
  prepare()
  const tmp = temporaryDirectory()
  const storeDir = path.join(tmp, 'store')

  await execa('node', [pnpmBin, 'add', 'lodash@4.17.19', `--store-dir=${storeDir}`])
  await execa('node', [pnpmBin, 'add', 'lodash@4.17.20', `--store-dir=${storeDir}`])

  {
    const output = await findHash.handler({
      pnpmHomeDir: '',
      storeDir,
    }, ['sha512-fXs1pWlUdqT2jkeoEJW/+odKZ2NwAyYkWea+plJKZI2xmhRKQi2e+nKGcClyDblgLwCLD912oMaua0+sTwwIrw=='])

    // The output contains colored package info and SQLite index keys (integrity\tpkgId)
    expect(output).toContain(PACKAGE_INFO_CLR('lodash'))
    expect(output).toContain(PACKAGE_INFO_CLR('4.17.19'))
    expect(output).toContain(PACKAGE_INFO_CLR('4.17.20'))
    expect(output).toContain('lodash@4.17.19')
    expect(output).toContain('lodash@4.17.20')
  }
})

test('print index file path with hash error', async () => {
  let err!: PnpmError
  try {
    const { config } = await getConfig({
      cliOptions: {},
      packageManager: {
        name: 'pnpm',
        version: '8.12.1',
      },
    })
    await findHash.handler(config as findHash.FindHashCommandOptions, ['sha512-fXs1pWlUdqT2j'])
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }

  expect(err.message).toBe('No package or index file matching this hash was found.')
  expect(err.code).toBe('ERR_PNPM_INVALID_FILE_HASH')
})
