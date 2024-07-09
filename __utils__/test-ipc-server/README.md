# @pnpm/test-ipc-server

The `TestIpcServer` is a simple Inter-Process Communication (IPC) server written specifically for usage in pnpm tests.

It's a simple wrapper around Node.js's builtin [_IPC support_](https://nodejs.org/api/net.html#ipc-support). Messages sent to the server are saved to a buffer that can be retrieved for assertions.

## Rationale

In the past, many pnpm tests contained scripts that wrote output to the same file. Writing to the same file concurrently causes race conditions resulting in flaky CI tests. The race conditions occur due to multiple processes reading a file, appending data, and writing the file back out. If two processes start at the same time and read the same input, one of the process's output would be overwritten by the other.

At the time of writing (December 2023), there's no great cross-platform way to append to a file atomically. From https://www.man7.org/linux/man-pages/man2/open.2.html

> `O_APPEND` may lead to corrupted files on NFS filesystems if more than one process appends data to a file at once. This is because NFS does not support appending to a file, so the client kernel has to simulate it, which can't be done without a race condition.

The `TestIpcServer` doesn't drop messages the same way since it's using Node.js's IPC mechanism that is specifically designed to handle multiple clients.

## Example

A common testing pattern in the pnpm repo is to ensure package scripts runs as expected or in particular orders.

```json
{
  "name": "@pnpm/example-test-fixture",
  "private": true,
  "scripts": {
    "build": "echo 'This script should run'"
  }
}
```

This can be tested through the `TestIpcServer`,

```ts
import { prepare } from '@pnpm/prepare'
import { createTestIpcServer } from '@pnpm/test-ipc-server'

test('example test', async () => {
  await using server = await createTestIpcServer()
  prepare({
    scripts: {
      build: server.sendLineScript('this is a built script that should run'),
    },
  })

  await execa('node', [pnpmBin, 'run', 'build'])

  expect(server.getLines()).toStrictEqual(['this is a built script that should run'])
})
```

## License

MIT
