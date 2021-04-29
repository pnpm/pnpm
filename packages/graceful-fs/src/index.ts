import gfs from 'graceful-fs'
import { promisify } from 'util'

export default {
  createReadStream: gfs.createReadStream,
  readFile: promisify(gfs.readFile),
  writeFile: promisify(gfs.writeFile),
}