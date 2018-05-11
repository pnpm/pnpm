import diable = require('diable')
import path = require('path')

const pnpm = path.join(__dirname, 'bin', 'pnpm.js')

export default (storePath: string) => {
  return diable.daemonize(pnpm, ['server', 'start', '--store', storePath], {stdio: 'inherit'})
}
