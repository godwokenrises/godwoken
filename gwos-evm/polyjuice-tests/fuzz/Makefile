OS = Unknown
ifneq ($(shell uname -a | grep -i Darwin),)
	OS = MacOS
endif
ifneq ($(shell uname -a | grep -i Linux),)
	OS = Linux
endif
ifeq ($(OS),Unknown)
	echo "error: unsupported OS"; exit 1
endif

NPROC?=4
CC=clang
CXX=clang++
LLVM_PROFDATA=llvm-profdata
LLVM_COV=llvm-cov

DEPS := ../../deps
BUILD := build

SECP_DIR := $(DEPS)/secp256k1-fix
SECP256K1_SRC := $(SECP_DIR)/src/ecmult_static_pre_context.h

CFLAGS_SECP := -isystem $(SECP_DIR)/src -isystem $(SECP_DIR)
CFLAGS_CKB_STD = -I./ -I$(DEPS)/ckb-c-stdlib -I$(DEPS)/ckb-c-stdlib/molecule
CFLAGS_ETHASH := -I$(DEPS)/ethash/include -I$(DEPS)/ethash/lib/ethash -I$(DEPS)/ethash/lib/keccak -I$(DEPS)/ethash/lib/support
CFLAGS_EVMONE := -I$(DEPS)/evmone/lib/evmone -I$(DEPS)/evmone/include -I$(DEPS)/evmone/evmc/include -I$(DEPS)/evmone/evmc/tools/
CFLAGS_SMT := -I$(DEPS)/godwoken-scripts/c/deps/sparse-merkle-tree/c
CFLAGS_GODWOKEN := -I$(DEPS)/godwoken-scripts/c
CFLAGS_MBEDTLS := -I$(DEPS)/mbedtls/include
CFLAGS_CRYPTO_ALGORITHMS := -I$(DEPS)/crypto-algorithms
CFLAGS_INTX := -I$(DEPS)/intx/lib/intx -I$(DEPS)/intx/include
CFLAGS := -Wall -O2 -I../../c -I../../c/ripemd160 $(CFLAGS_CKB_STD) $(CFLAGS_ETHASH) $(CFLAGS_EVMONE) $(CFLAGS_SMT) $(CFLAGS_GODWOKEN) $(CFLAGS_SECP) $(CFLAGS_MBEDTLS) $(CFLAGS_CRYPTO_ALGORITHMS) $(CFLAGS_INTX)
CXXFLAGS := $(CFLAGS) -std=c++1z
LDFLAGS := -Wl,--gc-sections
SANITIZER_FLAGS := -g -O1 -fsanitize=address,undefined -Wno-incompatible-pointer-types
LIMIT_ERROR := -ferror-limit=1

# TODO: read PROTOCOL_VERSION from deps/godwoken-scripts/c/Makefile
MOLC := moleculec
MOLC_VERSION := 0.7.2
PROTOCOL_VERSION := bf76c19c43e4e59cfaba871e722681bf7d3d6948
PROTOCOL_SCHEMA_URL := https://raw.githubusercontent.com/nervosnetwork/godwoken/${PROTOCOL_VERSION}/crates/types/schemas

ALL_OBJS := $(BUILD)/keccak.o $(BUILD)/keccakf800.o \
  $(BUILD)/execution_state.o $(BUILD)/evmc_hex.o $(BUILD)/baseline.o $(BUILD)/analysis.o $(BUILD)/instruction_metrics.o $(BUILD)/instruction_names.o $(BUILD)/execution.o $(BUILD)/instructions.o $(BUILD)/instructions_calls.o $(BUILD)/evmone.o \
  $(BUILD)/sha256.o $(BUILD)/memzero.o $(BUILD)/ripemd160.o $(BUILD)/bignum.o $(BUILD)/platform_util.o
BIN_DEPS := ../../c/contracts.h ../../c/sudt_contracts.h ../../c/other_contracts.h ../../c/polyjuice.h ../../c/polyjuice_utils.h $(BUILD)/secp256k1_data_info.h $(ALL_OBJS)
GENERATOR_DEPS := ../../c/generator/secp256k1_helper.h $(BIN_DEPS)
VALIDATOR_DEPS := ../../c/validator/secp256k1_helper.h $(BIN_DEPS)

CORPUS_DIR=corpus
COVERAGE_DIR=coverage
# TODO:
COVERAGE_FLAGS=-fprofile-instr-generate -fcoverage-mapping -I ../../c 
ifeq ($(OS),MacOS)
	COVERAGE_FLAGS+=-Wl,-U,_LLVMFuzzerCustomMutator -Wl,-U,_LLVMFuzzerInitialize
endif

EXTERNAL_HEADERS := $(DEPS)/ckb-c-stdlib-simulator-only/ckb_consts.h

#TODO: coverage
all: generate-protocol build/polyjuice_generator_fuzzer

build/polyjuice_generator_fuzzer: generate-protocol $(GENERATOR_DEPS)
	$(CXX) $(CFLAGS) $(LDFLAGS) $(SANITIZER_FLAGS) $(LIMIT_ERROR) -fsanitize=fuzzer -Ibuild -o $@ polyjuice_generator_fuzzer.cc $(ALL_OBJS)
build/polyjuice_generator_fuzzer_log: generate-protocol $(GENERATOR_DEPS)
	$(CXX) $(CFLAGS) $(LDFLAGS) $(SANITIZER_FLAGS) $(LIMIT_ERROR) -fsanitize=fuzzer -Ibuild -o $@ polyjuice_generator_fuzzer.cc $(ALL_OBJS) -DPOLYJUICE_DEBUG_LOG

###
# TODO:
show: $(COVERAGE_DIR)/fuzzer.profdata
	$(LLVM_COV) show --instr-profile=$(COVERAGE_DIR)/fuzzer.profdata smt_coverage
# TODO: report
report: $(COVERAGE_DIR)/fuzzer.profdata coverage $(EXTERNAL_HEADERS)
	$(LLVM_COV) report --show-functions --instr-profile=$(COVERAGE_DIR)/fuzzer.profdata smt_coverage $(EXTERNAL_HEADERS)
# TODO:
coverage: $(EXTERNAL_HEADERS)
	clang $(COVERAGE_FLAGS) smt_coverage.c smt_fuzzer.c -o smt_coverage

# start-fuzzer: fuzzer
# 	./smt_fuzzer -max_len=800000 -workers=$(NPROC) -jobs=$(NPROC) corpus

# start-fuzzer2: fuzzer
# 	./smt_fuzzer -max_len=800000 corpus

clean:
	rm -rf $(BUILD)/*

###

build/generator: ../../c/generator.c $(GENERATOR_DEPS)
	cd $(SECP_DIR) && (git apply workaround-fix-g++-linking.patch || true) && cd - # apply patch
	$(CXX) $(SANITIZER_FLAGS) $(CFLAGS) $(LDFLAGS) -Ibuild -o $@ ../../c/generator.c $(ALL_OBJS)
	$(OBJCOPY) --only-keep-debug $@ $@.debug
	$(OBJCOPY) --strip-debug --strip-all $@
	cd $(SECP_DIR) && (git apply -R workaround-fix-g++-linking.patch || true) && cd - # revert patch
build/generator_log: ../../c/generator.c $(GENERATOR_DEPS)
	cd $(SECP_DIR) && (git apply workaround-fix-g++-linking.patch || true) && cd - # apply patch
	$(CXX) $(CFLAGS) $(LDFLAGS) $(SANITIZER_FLAGS) -Ibuild -o $@ ../../c/generator.c $(ALL_OBJS) -DPOLYJUICE_DEBUG_LOG
	$(OBJCOPY) --only-keep-debug $@ $@.debug
	$(OBJCOPY) --strip-debug --strip-all $@
	cd $(SECP_DIR) && (git apply -R workaround-fix-g++-linking.patch || true) && cd - # revert patch
build/validator: ../../c/validator.c $(VALIDATOR_DEPS)
	cd $(SECP_DIR) && (git apply workaround-fix-g++-linking.patch || true) && cd - # apply patch
	$(CXX) $(SANITIZER_FLAGS) $(CFLAGS) $(LDFLAGS) -Ibuild -o $@ ../../c/validator.c $(ALL_OBJS)
	$(OBJCOPY) --only-keep-debug $@ $@.debug
	$(OBJCOPY) --strip-debug --strip-all $@
	cd $(SECP_DIR) && (git apply -R workaround-fix-g++-linking.patch || true) && cd - # revert patch
build/validator_log: ../../c/validator.c $(VALIDATOR_DEPS)
	cd $(SECP_DIR) && (git apply workaround-fix-g++-linking.patch || true) && cd - # apply patch
	$(CXX) $(SANITIZER_FLAGS) $(CFLAGS) $(LDFLAGS) -Ibuild -o $@ ../../c/validator.c $(ALL_OBJS) -DPOLYJUICE_DEBUG_LOG
	$(OBJCOPY) --only-keep-debug $@ $@.debug
	$(OBJCOPY) --strip-debug --strip-all $@
	cd $(SECP_DIR) && (git apply -R workaround-fix-g++-linking.patch || true) && cd - # revert patch

build/test_contracts: generate-protocol ../../c/tests/test_contracts.c $(VALIDATOR_DEPS)
	cd $(SECP_DIR) && (git apply workaround-fix-g++-linking.patch || true) && cd - # apply patch
	${CXX} $(CFLAGS) $(LDFLAGS) $(SANITIZER_FLAGS) -Ibuild -o $@ ../../c/tests/test_contracts.c $(ALL_OBJS) -DFUZZING
	$(OBJCOPY) --only-keep-debug $@ $@.debug
	$(OBJCOPY) --strip-debug --strip-all $@
	cd $(SECP_DIR) && (git apply -R workaround-fix-g++-linking.patch || true) && cd - # revert patch

# test rlp_encode_sender_and_nonce
# RLP (Recursive Length Prefix)
build/test_rlp: generate-protocol ../../c/tests/test_rlp.c $(VALIDATOR_DEPS)
	cd $(SECP_DIR) && (git apply workaround-fix-g++-linking.patch || true) && cd - # apply patch
	$(CXX) $(CFLAGS) $(LDFLAGS) $(SANITIZER_FLAGS) -Ibuild -o $@ ../../c/tests/test_rlp.c $(ALL_OBJS)
	$(OBJCOPY) --only-keep-debug $@ $@.debug
	$(OBJCOPY) --strip-debug --strip-all $@
	cd $(SECP_DIR) && (git apply -R workaround-fix-g++-linking.patch || true) && cd - # revert patch

# evmone is a C++ implementation of the Ethereum Virtual Machine (EVM)
build/evmone.o: $(DEPS)/evmone/lib/evmone/evmone.cpp
	$(CXX) $(CXXFLAGS) $(LDFLAGS) -c -o $@ $< -DPROJECT_VERSION=\"0.5.0-dev\"
build/baseline.o: $(DEPS)/evmone/lib/evmone/baseline.cpp
	$(CXX) $(CXXFLAGS) $(LDFLAGS) -c -o $@ $<
build/analysis.o: $(DEPS)/evmone/lib/evmone/analysis.cpp
	$(CXX) $(CXXFLAGS) $(LDFLAGS) -c -o $@ $<
build/execution.o: $(DEPS)/evmone/lib/evmone/execution.cpp
	$(CXX) $(CXXFLAGS) $(LDFLAGS) -g -c -o $@ $<
build/instructions.o: $(DEPS)/evmone/lib/evmone/instructions.cpp
	$(CXX) $(CXXFLAGS) $(LDFLAGS) -c -o $@ $<
build/instruction_metrics.o: $(DEPS)/evmone/evmc/lib/instructions/instruction_metrics.c
	$(CXX) $(CXXFLAGS) $(LDFLAGS) -c -o $@ $<
build/instruction_names.o: $(DEPS)/evmone/evmc/lib/instructions/instruction_names.c
	$(CXX) $(CXXFLAGS) $(LDFLAGS) -c -o $@ $<
build/instructions_calls.o: $(DEPS)/evmone/lib/evmone/instructions_calls.cpp
	$(CXX) $(CXXFLAGS) $(LDFLAGS) -g -c -o $@ $<
build/evmc_hex.o: $(DEPS)/evmone/evmc/lib/hex/hex.cpp
	$(CXX) $(CXXFLAGS) $(LDFLAGS) -c -o $@ $<
build/execution_state.o: $(DEPS)/evmone/lib/evmone/execution_state.cpp
	$(CXX) $(CXXFLAGS) $(LDFLAGS) -c -o $@ $<

# new SHA-3 encryption standard - Keccak algorithm
build/keccak.o: $(DEPS)/ethash/lib/keccak/keccak.c build/keccakf800.o
	$(CC) $(CFLAGS) $(LDFLAGS) -c -o $@ $<
build/keccakf800.o: $(DEPS)/ethash/lib/keccak/keccakf800.c
	mkdir -p build
	$(CC) $(CFLAGS) $(LDFLAGS) -c -o $@ $<

build/memzero.o: ../../c/ripemd160/memzero.c
	$(CXX) $(CFLAGS) $(LDFLAGS) -c -o $@ $<
build/ripemd160.o: ../../c/ripemd160/ripemd160.c
	$(CXX) $(CFLAGS) $(LDFLAGS) -c -o $@ $<

# Mbed TLS is a C library that implements cryptographic primitives,
# X.509 certificate manipulation and the SSL/TLS and DTLS protocols.
build/platform_util.o: $(DEPS)/mbedtls/library/platform_util.c
	$(CC) $(CFLAGS) $(LDFLAGS) -c -o $@ $<
build/bignum.o: $(DEPS)/mbedtls/library/bignum.c
	$(CC) $(CFLAGS) $(LDFLAGS) -c -o $@ $<
	
build/sha256.o: $(DEPS)/crypto-algorithms/sha256.c
	$(CXX) $(CFLAGS) $(LDFLAGS) -c -o $@ $<

# Secp256k1 refers to the parameters of the elliptic curve used in Bitcoin's public-key cryptography, and is defined in Standards for Efficient Cryptography (SEC)
build/secp256k1_data_info.h: build/dump_secp256k1_data
	$<
build/dump_secp256k1_data: ../../c/dump_secp256k1_data.c $(SECP256K1_SRC)
	mkdir -p build
	gcc $(CFLAGS) -o $@ $<
$(SECP256K1_SRC):
	cd $(SECP_DIR) && (git apply -R workaround-fix-g++-linking.patch || true) && \
	chmod +x autogen.sh && ./autogen.sh && \
	./configure --with-bignum=no --enable-ecmult-static-precomputation --enable-endomorphism --enable-module-recovery && \
	make src/ecmult_static_pre_context.h src/ecmult_static_context.h

generate-protocol: check-moleculec-version build/blockchain.h build/godwoken.h
check-moleculec-version:
	test "$$(${MOLC} --version | awk '{ print $$2 }' | tr -d ' ')" = ${MOLC_VERSION}

build/blockchain.h: build/blockchain.mol
	${MOLC} --language c --schema-file $< > $@
build/godwoken.h: build/godwoken.mol
	${MOLC} --language c --schema-file $< > $@
build/blockchain.mol:
	mkdir -p build
	curl -L -o $@ ${PROTOCOL_SCHEMA_URL}/blockchain.mol
build/godwoken.mol:
	mkdir -p build
	curl -L -o $@ ${PROTOCOL_SCHEMA_URL}/godwoken.mol


#TODO:
#%.h:
#	ln -s $(CURDIR)/../$@ $(CURDIR)/$@

# %.profraw: coverage
# 	LLVM_PROFILE_FILE=$@ ./smt_coverage $(CORPUS_DIR)/*

%.profdata: %.profraw
	$(LLVM_PROFDATA) merge --sparse $< -o $@

.PHONY: all fuzzer coverage report

.PRECIOUS: $(COVERAGE_DIR)/fuzzer.profraw $(COVERAGE_DIR)/fuzzer.profdata
