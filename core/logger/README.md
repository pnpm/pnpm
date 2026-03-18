# @pnpm/logger

> Logger for pnpm

[![npm version](https://img.shields.io/npm/v/@pnpm/logger.svg)](https://www.npmjs.com/package/@pnpm/logger)

## Installation

```sh
pnpm add @pnpm/logger
```

## Usage

`@pnpm/logger` is mostly just a wrapper over [bole](https://www.npmjs.com/package/bole).
Logging is done the same way as in bole. To listed for logs, use `streamParser` or create
a new parser with `createStreamParser()`.

```typescript
import logger, {streamParser} from '@pnpm/logger'

logger.debug({ foo: 'bar' })

streamParser.on('data', msg => {
  // ...
})
```

## License

MIT
