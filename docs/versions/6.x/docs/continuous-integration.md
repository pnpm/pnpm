---
id: continuous-integration
title: Continuous Integration
---

pnpm can easily be used in various continuous integration systems.

## Travis

On [Travis CI], you can use pnpm for installing your dependencies by adding this
to your `.travis.yml` file:

```yaml title=".travis.yml"
cache:
  npm: false
  directories:
    - "~/.pnpm-store"
before_install:
  - curl -f https://get.pnpm.io/v6.16.js | node - add --global pnpm@6
  - pnpm config set store-dir ~/.pnpm-store
install:
  - pnpm install
```

[Travis CI]: https://travis-ci.org

## Semaphore

On [Semaphore], you can use pnpm for installing and caching your dependencies by
adding this to your `.semaphore/semaphore.yml` file:

```yaml title=".semaphore/semaphore.yml"
version: v1.0
name: Semaphore CI pnpm example
agent:
  machine:
    type: e1-standard-2
    os_image: ubuntu1804
blocks:
  - name: Install dependencies
    task:
      jobs:
        - name: pnpm install
          commands:
            - curl -f https://get.pnpm.io/v6.16.js | node - add --global pnpm@6
            - checkout
            - cache restore node-$(checksum pnpm-lock.yaml)
            - pnpm install
            - cache store node-$(checksum pnpm-lock.yaml) ~/.pnpm-store
```

[Semaphore]: https://semaphoreci.com

## AppVeyor

On [AppVeyor], you can use pnpm for installing your dependencies by adding this
to your `appveyor.yml`:

```yaml title="appveyor.yml"
install:
  - ps: Install-Product node $env:nodejs_version
  - curl -f https://get.pnpm.io/v6.16.js | node - add --global pnpm@6
  - pnpm install
```

[AppVeyor]: https://www.appveyor.com

## GitHub Actions

On GitHub Actions, you can use pnpm for installing and caching your dependencies
like so (belongs in `.github/workflows/NAME.yml`):

```yaml title=".github/workflows/NAME.yml"
name: pnpm Example Workflow
on:
  push:
jobs:
  build:
    runs-on: ubuntu-20.04
    strategy:
      matrix:
        node-version: [15]
    steps:
    - uses: actions/checkout@v3
    - uses: pnpm/action-setup@v2.0.1
      with:
        version: 6.20.3
    - name: Use Node.js ${{ matrix.node-version }}
      uses: actions/setup-node@v3
      with:
        node-version: ${{ matrix.node-version }}
        cache: 'pnpm'
    - name: Install dependencies
      run: pnpm install
```

:::note

Caching packages dependencies with `actions/setup-node@v2` requires you to install pnpm with version **6.10+**.

:::

## GitLab CI

On GitLab, you can use pnpm for installing and caching your dependencies
like so (belongs in `.gitlab-ci.yml`):

```yaml title=".gitlab-ci.yml"
stages:
  - build

build:
  stage: build
  image: node:14.16.0-buster
  before_script:
    - curl -f https://get.pnpm.io/v6.16.js | node - add --global pnpm@6
    - pnpm config set store-dir .pnpm-store
  script:
    - pnpm install # install dependencies
  cache:
    key: "$CI_COMMIT_REF_SLUG"
    paths:
      - .pnpm-store
```

## Bitbucket Pipelines

You can use pnpm for installing and caching your dependencies:

```yaml title=".bitbucket-pipelines.yml"
definitions:
  caches:
    pnpm: $BITBUCKET_CLONE_DIR/.pnpm-store

pipelines:
  pull-requests:
    "**":
      - step:
          name: Build and test
          image: node:14.16.0
          script:
            - curl -f https://get.pnpm.io/v6.16.js | node - add --global pnpm@6
            - pnpm install
            - pnpm run build # Replace with your build/test…etc. commands
          caches:
            - pnpm
```
