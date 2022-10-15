import { promisify } from 'util'
import gfs from 'graceful-fs'

export default { // eslint-disable-line
  createReadStream: gfs.createReadStream,
  readFile: promisify(gfs.readFile),
  writeFile: promisify(gfs.writeFile),
}
