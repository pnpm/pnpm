---
"pnpm": minor
"@pnpm/plugin-commands-installation": minor
---

In order to mute some types of peer dependency warnings, a new section in `package.json` may be used for declaring peer dependency warning rules. For example, the next configuration will turn off any warnings about missing `babel-loader` peer dependency and about `@angular/common`, when the wanted version of `@angular/common` is not v13.

```json
{
  "name": "foo",
  "version": "0.0.0",
  "pnpm": {
    "peerDependencyRules": {
      "ignoreMissing": ["babel-loader"],
      "allowedVersions": {
        "@angular/common": "13"
      }
    }
  }
}
```
