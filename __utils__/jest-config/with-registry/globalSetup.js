const { execFileSync } = require('child_process')
const path = require('path')
const getPort = require('get-port')
const { promisify } = require('util')
const kill = promisify(require('tree-kill'))

module.exports = async () => {
  if (!process.env.PNPM_REGISTRY_MOCK_PORT) {
    process.env.PNPM_REGISTRY_MOCK_PORT = (await getPort({ port: getPort.makeRange(7700, 7800) })).toString()
  }
  const { start, prepare } = require('@pnpm/registry-mock')
  prepare()
  // Verdaccio stopped working properly on Node.js 22.
  // You can test the issue by running:
  //   pnpm --filter=core run test test/install/auth.ts
  // registry-mock's `useNodeVersion` option passes `--use-node-version` to
  // pnpm, which pnpm 11 and newer don't have, so put the pinned Node.js first
  // on the PATH of the `node` it spawns instead.
  const nodePath = execFileSync('pnpm', ['dlx', 'node@runtime:20.16.0', '-p', 'process.execPath'], {
    encoding: 'utf8',
    shell: process.platform === 'win32',
  }).trim()
  const pathKey = Object.keys(process.env).find((key) => key.toUpperCase() === 'PATH') ?? 'PATH'
  const server = start({
    env: {
      [pathKey]: `${path.dirname(nodePath)}${path.delimiter}${process.env[pathKey]}`,
    },
    stdio: 'inherit',
    listen: process.env.PNPM_REGISTRY_MOCK_PORT,
  })
  let killed = false
  server.on('error', (err) => {
    console.log(err)
  })
  server.on('close', () => {
    if (!killed) {
      console.log('Error: The registry server was killed!')
      process.exit(1)
    }
  })
  global.killServer = () => {
    killed = true
    return kill(server.pid)
  }
}
