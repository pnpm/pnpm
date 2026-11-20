# @pnpm/os.available-parallelism

> The number of CPUs pnpm may use, capped by the cgroup CPU quota of its container

## Usage

```ts
import { getAvailableParallelism } from '@pnpm/os.available-parallelism'

const workers = Math.max(1, getAvailableParallelism() - 1)
```

## License

MIT
