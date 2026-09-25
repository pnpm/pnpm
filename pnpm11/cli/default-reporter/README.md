# @pnpm/cli.default-reporter

> The default reporter of pnpm

## Installation

```
pnpm add @pnpm/cli.default-reporter
```

## Usage

```ts
import { streamParser } from '@pnpm/logger'
import { initDefaultReporter } from '@pnpm/cli.default-reporter'

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

## `pnpm-render` bin

Installing this package exposes a `pnpm-render` bin that reads pnpm-shaped NDJSON from stdin and renders it through the default reporter. This lets external tools that emit `pnpm:*` log records reuse pnpm's renderer.

For example, [pacquet](https://github.com/pnpm/pacquet) emits the same wire format under `--reporter=ndjson` (to stderr), so its output can be piped through `pnpm-render`:

```sh
pacquet install --reporter=ndjson 2>&1 >/dev/null | pnpm-render
```

The redirect (`2>&1 >/dev/null`) is needed because pacquet writes the NDJSON stream to stderr.

An optional first positional argument sets the command name (defaults to `install`); pass it to match the verb the producer is running so command-specific renderers behave correctly:

```sh
pacquet add lodash --reporter=ndjson 2>&1 >/dev/null | pnpm-render add
```

## Style Guide

1. Never use blue or grey as font color as they are hard to read in many consoles.
   1. Use dim instead of grey
   1. Use cyan bright instead of blue
1. Don't hide the CLI cursor. (It is easier to never hide but it is really needed only when scripts are running.)
1. Don't use green and yellow to distinct something.

## License

[MIT](LICENSE)
