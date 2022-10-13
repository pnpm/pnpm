# @pnpm/default-reporter

> The default reporter of pnpm

## Installation

```
pnpm add @pnpm/default-reporter
```

## Usage

```ts
import { streamParser } from '@pnpm/logger'
import { initDefaultReporter } from '@pnpm/default-reporter'

const stopReporting = initDefaultReporter({
  context: {
    argv: [],
  },
  streamParser,
})

try {
  // calling some pnpm APIs
} finally {
  stopReporting()
}
```

## Style Guide

1. Never use blue or grey as font color as they are hard to read in many consoles.
   1. Use dim instead of grey
   1. Use cyan bright instead of blue
1. Don't hide the CLI cursor. (It is easier to never hide but it is really needed only when scripts are running.)
1. Don't use green and yellow to distinct something.

## License

[MIT](LICENSE)
