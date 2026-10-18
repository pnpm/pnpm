---
"pacquet": patch
---

Registry credentials and TLS settings passed with `--config`, such as `--config.//registry.example.com/:_authToken=<token>`, are now applied. They take precedence over the same settings in `.npmrc` files.
