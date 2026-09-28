# @pnpm/network.config

> Picks the setting that applies to a URL from settings keyed by registry URL

## Usage

```ts
import { pickSettingByUrl } from '@pnpm/network.config'

const settings = {
  '//registry.example.com/': { cert: 'cert', key: 'key' },
}

pickSettingByUrl(settings, 'https://registry.example.com/foo/-/foo-1.0.0.tgz')
//> { cert: 'cert', key: 'key' }
```

## License

MIT
