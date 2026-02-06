# @pnpm/fs.msgpack-file

> MessagePack file serialization utilities for pnpm

[![npm version](https://img.shields.io/npm/v/@pnpm/fs.msgpack-file.svg)](https://www.npmjs.com/package/@pnpm/fs.msgpack-file)

## Installation

```sh
pnpm add @pnpm/fs.msgpack-file
```

## Usage

```typescript
import {
  readMsgpackFile,
  readMsgpackFileSync,
  writeMsgpackFile,
  writeMsgpackFileSync,
} from '@pnpm/fs.msgpack-file'

// Async
await writeMsgpackFile('data.mpk', { foo: 'bar' })
const data = await readMsgpackFile('data.mpk')

// Sync
writeMsgpackFileSync('data.mpk', { foo: 'bar' })
const data = readMsgpackFileSync('data.mpk')
```

## Features

- Uses [msgpackr](https://github.com/kriszyp/msgpackr) for fast MessagePack serialization
- Supports `Map` and `Set` types natively
- Record structure optimization enabled for 2-3x faster decoding

## License

MIT
