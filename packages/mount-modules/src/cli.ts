import { getConfig } from '@pnpm/config'
import { promises as fs } from 'fs'
import path from 'path'

import { getStorePath } from '@pnpm/store-path'
import Fuse from 'fuse-native'
import { createFuseHandlers } from './createFuseHandlers'
(async () => { /* eslint-disable-line */
  const mnt = path.join(process.cwd(), 'node_modules')
  await fs.mkdir(mnt, { recursive: true })
  const { config } = await getConfig({
    cliOptions: {},
    packageManager: { name: '', version: '' },
  })
  const cafsDir = path.join(await getStorePath({
    pkgRoot: process.cwd(),
    storePath: config.storeDir,
    pnpmHomeDir: config.pnpmHomeDir,
  }), 'files')
  const fuse = new Fuse(mnt, await createFuseHandlers(process.cwd(), cafsDir), { debug: true })
  fuse.mount(function (err?: Error) {
    if (err != null) console.error(err)
  })

  process.once('SIGINT', function () {
    fuse.unmount((err?: Error) => {
      if (err != null) {
        console.log(`filesystem at ${fuse.mnt as string} not unmounted`, err)
      } else {
        console.log(`filesystem at ${fuse.mnt as string} unmounted`)
      }
    })
  })
})()
