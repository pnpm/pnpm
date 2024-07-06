# @pnpm/matcher

> A simple pattern matcher for pnpm

## Install

```
pnpm add @pnpm/matcher
```

## Usage

```ts
import { createMatcher } from '@pnpm/matcher'

const match = createMatcher(['eslint-*'])
match('eslint-plugin-foo')
//> true
```

## License

[MIT](LICENSE)
