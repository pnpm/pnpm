import getPort from 'get-port'
import { promisify } from 'util'
import treeKill from 'tree-kill'
const kill = promisify(treeKill)

export default async () => {
  if (!process.env.PNPM_REGISTRY_MOCK_PORT) {
    process.env.PNPM_REGISTRY_MOCK_PORT = (await getPort({ from: 7700, to: 7800 })).toString()
  }
  const { start, prepare } = await import('@pnpm/registry-mock')
  prepare()
  const server = start({
    // Verdaccio stopped working properly on Node.js 22.
    // You can test the issue by running:
    //   pnpm --filter=core run test test/install/auth.ts
    useNodeVersion: '20.16.0',
    stdio: 'inherit',
    listen: process.env.PNPM_REGISTRY_MOCK_PORT,
  })
  // Unref the server and its stdio so that the Verdaccio child process does
  // not prevent Jest from exiting.  With Jest 30 worker threads the main
  // thread and worker thread share the same event loop, so ref'd handles
  // from globalSetup keep the whole process alive after tests complete.
  // globalTeardown still properly kills the server via tree-kill.
  server.unref()
  if (server.stdout) server.stdout.unref()
  if (server.stderr) server.stderr.unref()
  if (server.stdin) server.stdin.unref()
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
