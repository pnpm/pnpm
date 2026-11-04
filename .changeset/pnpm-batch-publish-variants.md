---
"pacquet": minor
---

`pnpm -r publish --batch` publishes packages that declare `publishConfig.variants` to a registry declared `serverType: pnpr`. pnpm first stages the archives it packed for every package. An archive another job uploaded with `--upload-variant` is not uploaded again: pnpm checks that the registry still holds it. The batch then commits every archive by reference.
