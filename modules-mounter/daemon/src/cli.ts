import '@total-typescript/ts-reset'

import path from 'node:path'
import { promises as fs } from 'node:fs'

import { getConfig } from '@pnpm/config'
import { getStorePath } from '@pnpm/store-path'

import Fuse from 'fuse-native'

import { createFuseHandlers } from './createFuseHandlers'

;(async (): Promise<void> => {
  const mnt = path.join(process.cwd(), 'node_modules')

  await fs.mkdir(mnt, { recursive: true })

  const { config } = await getConfig({
    cliOptions: {},
    packageManager: { name: '', version: '' },
  })

  const cafsDir = path.join(
    await getStorePath({
      pkgRoot: process.cwd(),
      storePath: config.storeDir,
      pnpmHomeDir: config.pnpmHomeDir,
    }),
    'files'
  )

  const fuse = new Fuse(mnt, await createFuseHandlers(process.cwd(), cafsDir), {
    debug: true,
  })

  fuse.mount((err?: Error | undefined): void => {
    if (err != null) console.error(err)
  })

  process.once('SIGINT', (): void => {
    fuse.unmount((err?: Error | undefined) => {
      if (typeof err === 'undefined') {
        console.log(`filesystem at ${fuse.mnt as string} unmounted`)
      } else {
        console.log(`filesystem at ${fuse.mnt as string} not unmounted`, err)
      }
    })
  })
})()
