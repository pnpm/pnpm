import { promisify } from 'util'
import gfs from 'graceful-fs'

export default {
  createReadStream: gfs.createReadStream,
  readFile: promisify(gfs.readFile),
  writeFile: promisify(gfs.writeFile),
}