/// <reference lib="esnext.disposable" />
import execa from 'execa'
import fs from 'fs'
import net from 'net'
import path from 'path'
import { setTimeout } from 'timers/promises'
import { promisify } from 'util'
import { prepare } from '@pnpm/prepare'
import { createTestIpcServer } from '@pnpm/test-ipc-server'

const pnpmBin = path.join(__dirname, '../../../pnpm/bin/pnpm.cjs')

describe('TestEchoServer', () => {
  describe('lifecycle', () => {
    it('cleans up through Symbol.asyncDispose', async () => {
      let listenPath: string

      {
        await using server = await createTestIpcServer()
        listenPath = server.listenPath
        await expect(fs.promises.access(server.listenPath)).resolves.not.toThrow()
      }

      // The Symbol.asyncDispose method should have been called by this point and
      // removed the listening file.
      await expect(fs.promises.access(listenPath)).rejects.toThrow('ENOENT')
    })

    it('throws if another server is listening on same socket', async () => {
      await using server = await createTestIpcServer()
      await expect(createTestIpcServer(server.listenPath)).rejects.toThrow('EADDRINUSE')
    })
  })

  describe('message handling', () => {
    it('receives messages', async () => {
      await using server = await createTestIpcServer()
      await using client = await createClient(server.listenPath)
      await client.sendLine('hello')
      await client.sendLine('world')

      // Wait a short amount of time for the server to handle incoming messages.
      await setTimeout(50)
      expect(server.getBuffer()).toStrictEqual('hello\nworld\n')
      expect(server.getLines()).toStrictEqual(['hello', 'world'])
    })

    it('clears messages', async () => {
      await using server = await createTestIpcServer()
      await using client = await createClient(server.listenPath)
      await client.sendLine('hello')
      await client.sendLine('world')

      // Wait a short amount of time for the server to handle incoming messages.
      await setTimeout(50)
      expect(server.getLines()).toStrictEqual(['hello', 'world'])

      server.clear()

      expect(server.getLines()).toStrictEqual([])
    })
  })

  describe('generated scripts', () => {
    it('generates working send message script', async () => {
      await using server = await createTestIpcServer()

      prepare({
        scripts: {
          build: server.sendLineScript('build script'),
        },
      })

      await execa('node', [pnpmBin, 'run', 'build'])

      expect(server.getLines()).toStrictEqual(['build script'])
    })

    it('send message script works with &&', async () => {
      await using server = await createTestIpcServer()

      prepare({
        scripts: {
          build: `${server.sendLineScript('message1')} && ${server.sendLineScript('message2')}`,
        },
      })

      await execa('node', [pnpmBin, 'run', 'build'])

      expect(server.getLines()).toStrictEqual(['message1', 'message2'])
    })

    it('generates working stdin script', async () => {
      await using server = await createTestIpcServer()

      prepare({
        scripts: {
          build: `node -e "process.stdout.write('build script')" | ${server.generateSendStdinScript()}`,
        },
      })

      await execa('node', [pnpmBin, 'run', 'build'])

      expect(server.getLines()).toStrictEqual(['build script'])
    })
  })

  it('has working client binary', async () => {
    const project = prepare({
      scripts: {
        build: "node -e \"process.stdout.write('build script')\" | test-ipc-server-client ./test.sock",
      },
    })

    await using server = await createTestIpcServer(path.join(project.dir(), './test.sock'))

    await execa('node', [pnpmBin, 'run', 'build'])

    expect(server.getLines()).toStrictEqual(['build script'])
  })
})

interface TestClient extends AsyncDisposable {
  sendLine: (message: string) => Promise<void>
}

function createClient (handle: string): Promise<TestClient> {
  const client = net.connect(handle)

  const write = promisify(client.write).bind(client)
  const destroy = promisify(client.destroy).bind(client)

  return new Promise((resolve, reject) => {
    client.once('error', reject)
    client.once('ready', () => {
      resolve({
        sendLine: (message: string) => write(message + '\n'),
        [Symbol.asyncDispose]: async () => {
          await destroy(undefined)
        },
      })
    })
  })
}
