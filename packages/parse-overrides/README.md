# @pnpm/parse-overrides

> Parse overrides

## Installation

```
pnpm add @pnpm/parse-overrides
```

## Usage

```ts
import { parseOverrides } from '@pnpm/parse-overrides'

parseOverrides({
  'foo': '^1.0.0',
  'quux': 'npm:@myorg/quux@^1.0.0',
  'bar@^2.1.0': '3.0.0',
  'qar@1>zoo': '2',
})
// Returns an array of parsed overrides
```

## License

[MIT](LICENSE)
