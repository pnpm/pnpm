import { promisify } from 'util'
import gfs from 'graceful-fs'

export default { // eslint-disable-line
  copyFile: promisify(gfs.copyFile),
  copyFileSync: gfs.copyFileSync,
  createReadStream: gfs.createReadStream,
  link: promisify(gfs.link),
  linkSync: gfs.linkSync,
  readFile: promisify(gfs.readFile),
  readFileSync: gfs.readFileSync,
  readdirSync: gfs.readdirSync,
  stat: promisify(gfs.stat),
  statSync: gfs.statSync,
  unlinkSync: gfs.unlinkSync,
  writeFile: promisify(gfs.writeFile),
  writeFileSync: gfs.writeFileSync,
}
