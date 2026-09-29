---
id: continuous-integration
title: Continuous Integration
---

pnpm can easily be used in various continuous integration systems.

## Installing pnpm

Outside of GitHub Actions, which has [its own action](#github-actions), install pnpm
with the standalone script:

```sh
curl -fsSL https://get.pnpm.io/install.sh | sh -
```

Two things make this convenient in CI:

- **It needs no Node.js.** pnpm is a self-contained executable, and it can install
  the runtime for you afterwards with `pnpm runtime set node lts -g`, so a job
  does not need a Node.js image to begin with.
- **It follows the project's version.** The bootstrap installs the newest stable
  pnpm, then, if your `package.json` has a `packageManager` or
  `devEngines.packageManager` field, pnpm switches to that version on first use,
  so the pipeline does not pin a version in two places.

The script installs into `PNPM_HOME` and appends to a shell profile, which a CI
job never reloads. Declare `PNPM_HOME` and put `$PNPM_HOME/bin` on `PATH`
yourself, using whatever mechanism the platform provides — the examples below
show it for each one.

:::note

Earlier versions of this page used Corepack. Corepack installs a JavaScript shim
in place of pnpm, so every `pnpm` call starts Node.js to run the shim before pnpm
itself starts — a cost paid on each invocation, and CI jobs make many of them.
Installing pnpm itself avoids that entirely.

:::

:::note

In all the provided configuration files the store is cached. However, this is not required, and it is not guaranteed that caching the store will make installation faster. So feel free to not cache the pnpm store in your job.

:::

:::tip Cache the metadata cache too

Since v11.22.0, [`pnpm cache path`](./cli/cache-path.md) prints the directory pnpm
uses for its metadata cache, so a job can cache it without mirroring pnpm's own
path resolution. That directory also holds the lockfile verification log, which
lets a job skip re-checking an unchanged lockfile against the configured
[supply-chain policies](./supply-chain-security.md) — the dominant cost of an
install in CI once the store is warm.

:::

:::important

Only cache pnpm's store and cache directories in locations writable by trusted jobs. Do not let untrusted CI jobs write to a store or metadata cache that trusted jobs later restore. These directories are trusted caches; see the [`storeDir`](./settings/store.md#storedir) and [`cacheDir`](./settings/other.md#cachedir) settings for details.

:::

:::tip Lockfile behavior in CI

When pnpm detects that it is running in CI, it switches to frozen-lockfile mode automatically. Since v11, pnpm also fails on incompatible lockfiles in CI — if the lockfile was written by a newer pnpm major version, the install will error out instead of silently rewriting it. Upgrade your CI pnpm version to match the one used to generate the lockfile.

:::

## AppVeyor

On [AppVeyor], you can use pnpm for installing your dependencies by adding this
to your `appveyor.yml`:

```yaml title="appveyor.yml"
environment:
  PNPM_HOME: C:\pnpm

install:
  - ps: $env:PATH = "$env:PNPM_HOME;$env:PNPM_HOME\bin;$env:PATH"
  - ps: Invoke-WebRequest https://get.pnpm.io/install.ps1 -UseBasicParsing | Invoke-Expression
  - ps: pnpm install
```

pnpm brings its own runtime, so `Install-Product node` is no longer needed. Add
`pnpm runtime set node lts -g` if your build scripts call `node` directly.

[AppVeyor]: https://www.appveyor.com

## Azure Pipelines

On Azure Pipelines, you can use pnpm for installing and caching your dependencies by adding this to your `azure-pipelines.yml`:

```yaml title="azure-pipelines.yml"
variables:
  pnpm_config_cache: $(Pipeline.Workspace)/.pnpm-store
  PNPM_HOME: $(Pipeline.Workspace)/.pnpm

steps:
  - task: Cache@2
    inputs:
      key: 'pnpm | "$(Agent.OS)" | pnpm-lock.yaml'
      path: $(pnpm_config_cache)
    displayName: Cache pnpm

  - script: |
      curl -fsSL https://get.pnpm.io/install.sh | sh -
      echo "##vso[task.prependpath]$(PNPM_HOME)/bin"
      "$(PNPM_HOME)/bin/pnpm" config set store-dir $(pnpm_config_cache)
    displayName: "Setup pnpm"

  - script: |
      pnpm install
      pnpm run build
    displayName: "pnpm install and build"
```

`task.prependpath` puts pnpm on `PATH` for the steps that follow; within the step
that installs it, call it by its full path.

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
          image: debian:stable-slim
          script:
            - apt-get update && apt-get install -y --no-install-recommends ca-certificates curl
            - export PNPM_HOME="$HOME/.local/share/pnpm"
            - export PATH="$PNPM_HOME/bin:$PATH"
            - curl -fsSL https://get.pnpm.io/install.sh | sh -
            - pnpm config set store-dir "$BITBUCKET_CLONE_DIR/.pnpm-store"
            - pnpm runtime set node lts -g
            - pnpm install
            - pnpm run build # Replace with your build/test…etc. commands
          caches:
            - pnpm
```

All the lines of a step run in one shell, so `export` carries to the ones that
follow, and `store-dir` is pointed at the directory the cache saves.

This example starts from a plain image and lets pnpm install Node.js. Keep your
existing `node` image and drop the `pnpm runtime set` line if you would rather
not change it.

## CircleCI

On CircleCI, you can use pnpm for installing and caching your dependencies by adding this to your `.circleci/config.yml`:

```yaml title=".circleci/config.yml"
version: 2.1

jobs:
  build: # this can be any name you choose
    docker:
      - image: node:24
    resource_class: large
    parallelism: 10
    environment:
      PNPM_HOME: /root/.local/share/pnpm

    steps:
      - checkout
      - restore_cache:
          name: Restore pnpm Package Cache
          keys:
            - pnpm-packages-{{ checksum "pnpm-lock.yaml" }}
      - run:
          name: Install pnpm package manager
          command: |
            curl -fsSL https://get.pnpm.io/install.sh | sh -
            echo 'export PATH="$PNPM_HOME/bin:$PATH"' >> "$BASH_ENV"
            "$PNPM_HOME/bin/pnpm" config set store-dir .pnpm-store
      - run:
          name: Install Dependencies
          command: |
            pnpm install
      - save_cache:
          name: Save pnpm Package Cache
          key: pnpm-packages-{{ checksum "pnpm-lock.yaml" }}
          paths:
            - .pnpm-store
```

CircleCI sources `$BASH_ENV` before every step, which is how pnpm reaches the
steps that follow.

## GitHub Actions

On GitHub Actions, you can use pnpm for installing and caching your dependencies
like so (belongs in `.github/workflows/NAME.yml`):

```yaml title=".github/workflows/NAME.yml"
name: pnpm Example Workflow
on:
  push:

jobs:
  build:
    runs-on: ubuntu-24.04
    strategy:
      matrix:
        node-version: [24]
    steps:
      - uses: actions/checkout@v6
      - name: Install pnpm and Node.js
        uses: pnpm/setup@c9883cc79df532ad1a7b81bf9ab944ceb090d65c # v2.0.0
        with:
          runtime: node@${{ matrix.node-version }}
          cache: true
```

[`pnpm/setup`][pnpm-setup] installs pnpm, then uses it to install the requested
runtime, so no separate `actions/setup-node` step is needed. It also runs `pnpm
install` for you, and `cache: true` caches the pnpm store between runs. Set
`install: false` if you would rather run the install yourself.

The pnpm version comes from the `packageManager` or `devEngines.packageManager`
field of your `package.json`, so the workflow never needs updating when you
change it. Add a `version` input if your `package.json` declares neither.

[pnpm-setup]: https://github.com/pnpm/setup

## GitLab CI

On GitLab, you can use pnpm for installing and caching your dependencies
like so (belongs in `.gitlab-ci.yml`):

```yaml title=".gitlab-ci.yml"
stages:
  - build

build:
  stage: build
  image: node:24.14.1
  variables:
    PNPM_HOME: "$CI_PROJECT_DIR/.pnpm"
  before_script:
    - export PATH="$PNPM_HOME/bin:$PATH"
    - curl -fsSL https://get.pnpm.io/install.sh | sh -
    - pnpm config set store-dir .pnpm-store
  script:
    - pnpm install # install dependencies
  cache:
    key:
      files:
        - pnpm-lock.yaml
    paths:
      - .pnpm-store
```

## Jenkins

You can use pnpm for installing and caching your dependencies:

```title="Jenkinsfile"
pipeline {
    agent {
        docker {
            image 'node:lts-bookworm-slim'
            args '-p 3000:3000'
        }
    }
    environment {
        PNPM_HOME = "${WORKSPACE}/.pnpm"
        PATH = "${PNPM_HOME}/bin:${PATH}"
    }
    stages {
        stage('Build') {
            steps {
                sh 'curl -fsSL https://get.pnpm.io/install.sh | sh -'
                sh 'pnpm install'
            }
        }
    }
}
```

Each `sh` step runs in its own shell, so `PATH` is set in the pipeline's
`environment` block rather than exported inside a step.

## Semaphore

On [Semaphore], you can use pnpm for installing and caching your dependencies by
adding this to your `.semaphore/semaphore.yml` file:

```yaml title=".semaphore/semaphore.yml"
version: v1.0
name: Semaphore CI pnpm example
agent:
  machine:
    type: e1-standard-2
    os_image: ubuntu2404
blocks:
  - name: Install dependencies
    task:
      env_vars:
        - name: PNPM_HOME
          value: /home/semaphore/.local/share/pnpm
      jobs:
        - name: pnpm install
          commands:
            - export PATH="$PNPM_HOME/bin:$PATH"
            - curl -fsSL https://get.pnpm.io/install.sh | sh -
            - checkout
            - cache restore node-$(checksum pnpm-lock.yaml)
            - pnpm install
            - cache store node-$(checksum pnpm-lock.yaml) $(pnpm store path)
```

[Semaphore]: https://semaphoreci.com

## Travis

On [Travis CI], you can use pnpm for installing your dependencies by adding this
to your `.travis.yml` file:

```yaml title=".travis.yml"
cache:
  npm: false
  directories:
    - "~/.pnpm-store"
env:
  global:
    - PNPM_HOME="$HOME/.local/share/pnpm"
    - PATH="$PNPM_HOME/bin:$PATH"
before_install:
  - curl -fsSL https://get.pnpm.io/install.sh | sh -
  - pnpm config set store-dir ~/.pnpm-store
install:
  - pnpm install
```

[Travis CI]: https://travis-ci.org
