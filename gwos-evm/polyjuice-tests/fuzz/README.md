# Polyjuice fuzz testing

## Build

### 1. normal build

```sh
make build/fuzzer
```

### 2. or build with debug log

```sh
make build/fuzzer_log
```

## Run

Simply just run with:
```sh
build/fuzzer
```

Or run `fuzzer_log`:
```sh
build/fuzzer_log
```

### Corpus and Seed

Feeding fuzz testing with some predefined testcases: Seed. (Optional)
And save to `corpus` folder if any good cases are generated during running.

```sh
build/fuzzer corpus seed
```

## Coverage Profile

To genreate a coverage profile, we need to set `LLVM_PROFILE_FILE` and `max_total_time` first.

```sh
LLVM_PROFILE_FILE="build/fuzzer.profraw" build/fuzzer corpus -max_total_time=10
```

### Generate .profdata

```sh
llvm-profdata merge -sparse build/fuzzer.profraw -o build/fuzzer.profdata
```

### Show coverage in detail (Optional)

```sh
llvm-cov show build/fuzzer -instr-profile=build/fuzzer.profdata --show-branches=count --show-expansions > log
```

### Report

```sh
llvm-cov report ./build/fuzzer -instr-profile=build/fuzzer.profdata
```
