# @pnpm/symlink-dependency

> Symlink a dependency to node_modules

## Installation

```
pnpm install @pnpm/symlink-dependency
```

## Usage

```ts
import symlinkDependency from '@pnpm/symlink-dependency'

await symlinkDependency('/home/src/foo', '/home/src/my-project/node_modules', 'foo')
//> { reused: false }
```

## License

MIT
