# @pnpm/config.matcher

> A simple pattern matcher for pnpm

## Install

```
pnpm add @pnpm/config.matcher
```

## Usage

```ts
import { createMatcher } from '@pnpm/config.matcher'

const match = createMatcher(['eslint-*'])
match('eslint-plugin-foo')
//> true
```

## License

[MIT](LICENSE)
