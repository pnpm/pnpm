# Developing libSQL API for Rust

Setting up the environment:

```sh
export LIBSQL_STATIC_LIB_DIR=$(pwd)/../../.libs
```

Building the APIs:

```sh
cargo build
```

Running the tests:

```sh
cargo test
```

Running the benchmarks:

```sh
cargo bench
```

Run benchmarks and generate flamegraphs:

```console
echo -1 | sudo tee /proc/sys/kernel/perf_event_paranoid
cargo bench --bench benchmark -- --profile-time=5
```
