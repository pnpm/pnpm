import diable = require('diable')

const pnpm = require.main!.filename

export default (storePath: string) => {
  return diable.daemonize(pnpm, ['server', 'start', '--store-dir', storePath], { stdio: 'inherit' })
}
