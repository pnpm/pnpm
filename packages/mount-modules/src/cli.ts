import { promises as fs } from 'fs'
import path from 'path'

import getStorePath from '@pnpm/store-path'
import Fuse from 'fuse-native'
import createFuseHandlers from './createFuseHandlers'
(async () => { /* eslint-disable-line */
  const mnt = path.join(process.cwd(), 'node_modules')
  await fs.mkdir(mnt, { recursive: true })
  const cafsDir = path.join(await getStorePath(process.cwd()), 'files')
  const fuse = new Fuse(mnt, await createFuseHandlers(process.cwd(), cafsDir), { debug: true })
  fuse.mount(function (err?: Error) {
    if (err) console.error(err)
  })

  process.once('SIGINT', function () {
    fuse.unmount((err?: Error) => {
      if (err) {
        console.log(`filesystem at ${fuse.mnt as string} not unmounted`, err)
      } else {
        console.log(`filesystem at ${fuse.mnt as string} unmounted`)
      }
    })
  })
})()
