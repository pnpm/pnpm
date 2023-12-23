/// <reference path="../../../__typings__/index.d.ts" />
import path from 'path'

import { prepare } from '@pnpm/prepare'
import { getConfig } from '@pnpm/config'
import { findHash } from '@pnpm/plugin-commands-store-inspecting'
import { type PnpmError } from '@pnpm/error'

import execa from 'execa'
import tempy from 'tempy'

const pnpmBin = path.join(__dirname, '../../../pnpm/bin/pnpm.cjs')

test('print index file path with hash', async () => {
  const { PACKAGE_INFO_CLR, INDEX_PATH_CLR } = findHash
  prepare()
  const tmp = tempy.directory()
  const storeDir = path.join(tmp, 'store')

  await execa('node', [pnpmBin, 'add', 'lodash@4.17.19', `--store-dir=${storeDir}`])
  await execa('node', [pnpmBin, 'add', 'lodash@4.17.20', `--store-dir=${storeDir}`])

  {
    const output = await findHash.handler({
      pnpmHomeDir: '',
      storeDir,
    }, ['sha512-fXs1pWlUdqT2jkeoEJW/+odKZ2NwAyYkWea+plJKZI2xmhRKQi2e+nKGcClyDblgLwCLD912oMaua0+sTwwIrw=='])

    expect(output).toBe(`${PACKAGE_INFO_CLR('lodash')}@${PACKAGE_INFO_CLR('4.17.19')}  ${INDEX_PATH_CLR('/24/dbddf17111f46417d2fdaa260b1a37f9b3142340e4145efe3f0937d77eb56c862d2a1d2901ca16271dc0d6335b0237c2346768a3ec1a3d579018f1fc5f7a0d-index.json')}
${PACKAGE_INFO_CLR('lodash')}@${PACKAGE_INFO_CLR('4.17.20')}  ${INDEX_PATH_CLR('/3e/585d15c8a594e20d7de57b362ea81754c011acb2641a19f1b72c8531ea39825896bab344ae616a0a5a824cb9a381df0b3cddd534645cf305aba70a93dac698-index.json')}
`)
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

  expect(err.code).toBe('ERR_PNPM_INVALID_FILE_HASH')
  expect(err.message).toBe('No package or index file matching this hash was found.')
})