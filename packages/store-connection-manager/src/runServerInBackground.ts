import diable = require('diable')

const pnpm = require.resolve('pnpm/bin/pnpm.js', { paths: [__dirname] })

export default (storePath: string) => {
  return diable.daemonize(pnpm, ['server', 'start', '--store-dir', storePath], { stdio: 'inherit' })
}
