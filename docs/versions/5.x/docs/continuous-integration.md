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
  - curl -f https://get.pnpm.io/v5.js | node - add --global pnpm@5
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
            - curl -f https://get.pnpm.io/v5.js | node - add --global pnpm@5
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
  - curl -f https://get.pnpm.io/v5.js | node - add --global pnpm@5
  - pnpm install
```

[AppVeyor]: https://www.appveyor.com

## Sail CI

On [Sail CI], you can use pnpm for installing your dependencies by adding this to your `.sail.yml` file:

```yaml title=".sail.yml"
install:
  image: znck/pnpm:latest
  command:
    - pnpm
  args:
    - install
```
To get the exact Node version and pnpm version you require you can always make
your own Docker image and push to [Docker Hub](https://hub.docker.com/).

[Sali CI]: https://sail.ci

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
    - uses: actions/checkout@v2
    - name: Use Node.js ${{ matrix.node-version }}
      uses: actions/setup-node@v2
      with:
        node-version: ${{ matrix.node-version }}
    - name: Cache .pnpm-store
      uses: actions/cache@v1
      with:
        path: ~/.pnpm-store
        key: ${{ runner.os }}-node${{ matrix.node-version }}-${{ hashFiles('**/pnpm-lock.yaml') }}
    - name: Install pnpm
      run: curl -f https://get.pnpm.io/v5.js | node - add --global pnpm@5
    - name: pnpm Build
      run: pnpm install
```

:::note

Using `actions/setup-node@v2` you need to install pnpm with [root permissions](https://github.com/actions/setup-node/issues/177), eg:`sudo npm install -g pnpm`. Alternatively, if you specify the Node.js version to use, pnpm may be installed with no priviledged user.

:::
