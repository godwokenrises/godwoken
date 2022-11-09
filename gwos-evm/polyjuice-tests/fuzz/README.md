# Polyjuice Fuzz Test

[![FuzzTest](https://github.com/Flouse/godwoken-polyjuice/actions/workflows/fuzz.yml/badge.svg?branch=fuzz-v2)](https://github.com/Flouse/godwoken-polyjuice/actions/workflows/fuzz.yml)

These two file were created to simulate `gw_syscalls`:
- polyjuice-tests/fuzz/ckb_syscalls.h
- polyjuice-tests/fuzz/mock_godwoken.hpp

## Polyjuice Generator Fuzzer
```bash
make build/polyjuice_generator_fuzzer
./build/polyjuice_generator_fuzzer corpus -max_total_time=6

# or fuzzing in debug mode
make build/polyjuice_generator_fuzzer_log
./build/polyjuice_generator_fuzzer_log corpus -max_total_time=2
```

### General Algorithm
```pseudo code
// pseudo code
Instrument program for code coverage
load pre-defined transactions such as contracts deploying and then execute run_polyjuice()
while(true) {
  Choose random input from corpus
  Mutate/populate input into transactions
  Execute run_polyjuice() and collect coverage
  If new coverage/paths are hit add it to corpus (corpus - directory with test-cases)
}
```

## test_contracts on x86 with [sanitizers](https://github.com/google/sanitizers)
```bash
make build/test_contracts
./build/test_contracts

make build/test_rlp
./build/test_rlp
```

## How to debug Polyjuice generator on x86?
1. Compile Polyjuice generator on x86
    ```bash
    cd fuzz
    make build/polyjuice_generator_fuzzer
    ```
2. Construct `pre_defined_test_case` in [polyjuice_generator_fuzzer.cc](./polyjuice_generator_fuzzer.cc)
3. Run `build/polyjuice_generator_fuzzer_log` with GDB debugger, see: [launch.json](../../.vscode/launch.json) 

## Coverage Report[WIP]
TBD

### Related materials
- https://llvm.org/docs/LibFuzzer.html
- [What makes a good fuzz target](https://github.com/google/fuzzing/blob/master/docs/good-fuzz-target.md)
- [Clang's source-based code coverage](https://clang.llvm.org/docs/SourceBasedCodeCoverage.html)
