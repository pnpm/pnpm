---
"pacquet": minor
---

`pnpm -r publish --batch` publishes packages that declare `publishConfig.variants` to a registry declared `serverType: pnpr`. pnpm first stages the archives it packed for every package. An archive another job uploaded with `--upload-variant` is not uploaded again: pnpm checks that the registry still holds it. The batch then commits every archive by reference. A batch with variants that would reference more than 256 archives on one registry fails with `ERR_PNPM_BATCH_PUBLISH_TOO_MANY_ARCHIVES`.
