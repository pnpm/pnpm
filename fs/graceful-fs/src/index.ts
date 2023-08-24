import { promisify } from 'util'
import gfs from 'graceful-fs'

export default { // eslint-disable-line
  copyFile: promisify(gfs.copyFile),
  createReadStream: gfs.createReadStream,
  link: promisify(gfs.link),
  readFile: promisify(gfs.readFile),
  stat: promisify(gfs.stat),
  writeFile: promisify(gfs.writeFile),
  writeFileSync: gfs.writeFileSync,
  readFileSync: gfs.readFileSync,
  unlinkSync: gfs.unlinkSync,
  linkSync: gfs.linkSync,
  statSync: gfs.statSync,
  copyFileSync: gfs.copyFileSync,
}
