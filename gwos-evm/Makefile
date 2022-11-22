SHELL=/bin/bash

# TARGET := riscv64-unknown-linux-gnu
TARGET := riscv64-unknown-elf
CC := $(TARGET)-gcc
CXX := $(TARGET)-g++
LD := $(TARGET)-gcc
OBJCOPY := $(TARGET)-objcopy

SECP_DIR := deps/secp256k1-fix
SECP256K1_SRC := $(SECP_DIR)/src/ecmult_static_pre_context.h
GW_SCRIPTS := deps/gwos/c

CFLAGS_CKB_STD = -Ideps/ckb-c-stdlib -Ideps/ckb-c-stdlib/molecule
CFLAGS_SECP := -isystem $(SECP_DIR)/src -isystem $(SECP_DIR)
CFLAGS_INTX := -Ideps/intx/lib/intx -Ideps/intx/include
CFLAGS_ETHASH := -Ideps/ethash/include -Ideps/ethash/lib/ethash -Ideps/ethash/lib/keccak -Ideps/ethash/lib/support
CFLAGS_CRYPTO_ALGORITHMS := -Ideps/crypto-algorithms
CFLAGS_MBEDTLS := -Ideps/mbedtls/include
CFLAGS_EVMONE := -Ideps/evmone/lib/evmone -Ideps/evmone/include -Ideps/evmone/evmc/include
CFLAGS_SMT := -I$(GW_SCRIPTS)/deps/sparse-merkle-tree/c
CFLAGS_GODWOKEN := -I$(GW_SCRIPTS)

# -O3: Higher level of optmization. Slower compile-time, better for production builds.
CFLAGS := -O3 -Ic/ripemd160 $(CFLAGS_INTX) $(CFLAGS_CKB_STD) $(CFLAGS_EVMONE) $(CFLAGS_ETHASH) $(CFLAGS_CRYPTO_ALGORITHMS) $(CFLAGS_MBEDTLS) $(CFLAGS_SMT) $(CFLAGS_GODWOKEN) $(CFLAGS_SECP)
CXXFLAGS := $(CFLAGS) -std=c++1z

# -Wl,<args> Pass the comma separated arguments in args to the linker(GNU linker)
# --gc-sections
#   This will perform a garbage collection of code and data never referenced.
#   together with -ffunction-sections and -fdata-sections
# -static
# 	On systems that support dynamic linking, this prevents linking with the 
#   shared libraries. On other systems, this option has no effect.
LDFLAGS := -Wl,-static -Wl,--gc-sections -fdata-sections -ffunction-sections -Wall

GENERATOR_FLAGS := -DGW_GENERATOR
VALIDATOR_FLAGS := -DGW_VALIDATOR

MOLC := moleculec
MOLC_VERSION := $(shell cat $(GW_SCRIPTS)/Makefile | egrep "MOLC_VERSION :=" | awk '{print $$3}')
PROTOCOL_VERSION := $(shell cat $(GW_SCRIPTS)/Makefile | egrep "PROTOCOL_VERSION :=" | awk '{print $$3}')
PROTOCOL_SCHEMA_URL := https://raw.githubusercontent.com/nervosnetwork/godwoken/${PROTOCOL_VERSION}/crates/types/schemas

ALL_OBJS := build/execution_state.o build/baseline.o build/analysis.o build/instruction_metrics.o build/instruction_names.o build/execution.o build/instructions.o build/instructions_calls.o build/evmone.o \
  build/keccak.o build/keccakf800.o \
  build/sha256.o build/memzero.o build/ripemd160.o build/bignum.o build/platform_util.o
BIN_DEPS := c/contracts.h c/sudt_contracts.h c/other_contracts.h c/polyjuice.h c/polyjuice_utils.h build/secp256k1_data_info.h $(ALL_OBJS)
GENERATOR_DEPS := c/generator/secp256k1_helper.h $(BIN_DEPS)
VALIDATOR_DEPS := c/validator/secp256k1_helper.h $(BIN_DEPS)

# docker pull nervos/ckb-riscv-gnu-toolchain:gnu-bionic-20191012
# BUILDER_DOCKER := nervos/ckb-riscv-gnu-toolchain@sha256:aae8a3f79705f67d505d1f1d5ddc694a4fd537ed1c7e9622420a470d59ba2ec3
# docker pull nervos/ckb-riscv-gnu-toolchain:bionic-20190702
BUILDER_DOCKER := nervos/ckb-riscv-gnu-toolchain@sha256:7b168b4b109a0f741078a71b7c4dddaf1d283a5244608f7851f5714fbad273ba
# TODO: update to BUILDER_DOCKER := nervos/ckb-riscv-gnu-toolchain:bionic-20190702-newlib-debug-symbols
# TODO: update to nervos/ckb-riscv-gnu-toolchain:bionic-20211214 => need more tests

all: build/blockchain.h build/godwoken.h \
  build/test_contracts build/test_rlp build/test_ripemd160 build/test_calc_fee \
  build/generator build/validator \
  build/generator_log build/validator_log

all-via-docker: generate-protocol fetch-gw-scripts
	mkdir -p build
	docker run --rm -v `pwd`:/code -w /code ${BUILDER_DOCKER} make
	make patch-generator && make patch-generator_log
log-version-via-docker: generate-protocol
	mkdir -p build
	docker run --rm -v `pwd`:/code -w /code ${BUILDER_DOCKER} bash -c "make build/generator_log && make build/validator_log"

all-via-docker-in-debug-mode: generate-protocol
	docker run --rm -v `pwd`:/code -w /code ${BUILDER_DOCKER} make all-in-debug-mode
# Be aware that a given prerequisite will only be built once per invocation of make, at most.
all-in-debug-mode: LDFLAGS := -g # only use -O0 to decrease compile time while coding and debugging (O0 compile time: 1m58s)
all-in-debug-mode: CFLAGS += -DPOLYJUICE_DEBUG_LOG
all-in-debug-mode: all

clean-via-docker:
	mkdir -p build
	docker run --rm -v `pwd`:/code -w /code ${BUILDER_DOCKER} make clean

dist: clean-via-docker all-via-docker

CKB_BIN_PATCHER := deps/ckb-binary-patcher/target/release/ckb-binary-patcher
build/ckb-binary-patcher:
	cd deps && [ -d "ckb-binary-patcher" ] \
	  || ( \
		echo "fetch ckb-binary-patcher with [Remove atomic instructions] feature" && \
		git clone -b master https://github.com/nervosnetwork/ckb-binary-patcher.git)
	[ -f ${CKB_BIN_PATCHER} ] \
	  || ( \
		echo "build ckb-binary-patcher" && \
		cd deps/ckb-binary-patcher && \
		git checkout b9489de4b3b9d59bc29bce945279bc6f28413113 && \
		cargo build --release)
patch-generator: build/ckb-binary-patcher
	${CKB_BIN_PATCHER} --remove-a -i build/generator -o build/generator.aot
	mv build/generator build/generator.asm
	cp build/generator.aot build/generator
patch-generator_log: build/ckb-binary-patcher
	${CKB_BIN_PATCHER} --remove-a -i build/generator_log -o build/generator_log.aot
	mv build/generator_log build/generator_log.asm
	cp build/generator_log.aot build/generator_log
# patch-validator: build/ckb-binary-patcher
# 	${CKB_BIN_PATCHER} --remove-a -i build/validator -o build/validator.aot
# patch-validator_log: build/ckb-binary-patcher
# 	${CKB_BIN_PATCHER} --remove-a -i build/validator_log -o build/validator_log.aot
# patch-test_contracts: build/ckb-binary-patcher
# 	${CKB_BIN_PATCHER} --remove-a -i build/test_contracts -o build/test_contracts.aot

build/generator: c/generator.c $(GENERATOR_DEPS)
	cd $(SECP_DIR) && (git apply workaround-fix-g++-linking.patch || true) && cd - # apply patch
	$(CXX) $(CFLAGS) $(LDFLAGS) -Ibuild -o $@ c/generator.c $(ALL_OBJS)
	$(OBJCOPY) --only-keep-debug $@ $@.debug
	$(OBJCOPY) --strip-debug --strip-all $@
	cd $(SECP_DIR) && (git apply -R workaround-fix-g++-linking.patch || true) && cd - # revert patch

build/validator: c/validator.c $(VALIDATOR_DEPS)
	cd $(SECP_DIR) && (git apply workaround-fix-g++-linking.patch || true) && cd - # apply patch
	$(CXX) $(CFLAGS) $(LDFLAGS) -Ibuild -o $@ c/validator.c $(ALL_OBJS)
	$(OBJCOPY) --only-keep-debug $@ $@.debug
	$(OBJCOPY) --strip-debug --strip-all $@
	cd $(SECP_DIR) && (git apply -R workaround-fix-g++-linking.patch || true) && cd - # revert patch

build/generator_log: c/generator.c $(GENERATOR_DEPS)
	cd $(SECP_DIR) && (git apply workaround-fix-g++-linking.patch || true) && cd - # apply patch
	$(CXX) $(CFLAGS) $(LDFLAGS) -Ibuild -o $@ c/generator.c $(ALL_OBJS) -DPOLYJUICE_DEBUG_LOG
#	If we need the whole one for performance analysis, don't separate the executable here
	$(OBJCOPY) --only-keep-debug $@ $@.debug
	$(OBJCOPY) --strip-debug --strip-all $@
	cd $(SECP_DIR) && (git apply -R workaround-fix-g++-linking.patch || true) && cd - # revert patch

build/validator_log: c/validator.c $(VALIDATOR_DEPS)
	cd $(SECP_DIR) && (git apply workaround-fix-g++-linking.patch || true) && cd - # apply patch
	$(CXX) $(CFLAGS) $(LDFLAGS) -Ibuild -o $@ c/validator.c $(ALL_OBJS) -DPOLYJUICE_DEBUG_LOG
#	If we need the whole one for performance analysis, don't separate the executable here
	$(OBJCOPY) --only-keep-debug $@ $@.debug
	$(OBJCOPY) --strip-debug --strip-all $@
	cd $(SECP_DIR) && (git apply -R workaround-fix-g++-linking.patch || true) && cd - # revert patch

build/test_contracts: c/tests/test_contracts.c $(VALIDATOR_DEPS)
	cd $(SECP_DIR) && (git apply workaround-fix-g++-linking.patch || true) && cd - # apply patch
	$(CXX) $(CFLAGS) $(LDFLAGS) -Ibuild -o $@ c/tests/test_contracts.c $(ALL_OBJS)
	$(OBJCOPY) --only-keep-debug $@ $@.debug
	$(OBJCOPY) --strip-debug --strip-all $@
	cd $(SECP_DIR) && (git apply -R workaround-fix-g++-linking.patch || true) && cd - # revert patch

build/test_rlp: c/tests/test_rlp.c $(VALIDATOR_DEPS)
	cd $(SECP_DIR) && (git apply workaround-fix-g++-linking.patch || true) && cd - # apply patch
	$(CXX) $(CFLAGS) $(LDFLAGS) -Ibuild -o $@ c/tests/test_rlp.c $(ALL_OBJS)
	$(OBJCOPY) --only-keep-debug $@ $@.debug
	$(OBJCOPY) --strip-debug --strip-all $@
	cd $(SECP_DIR) && (git apply -R workaround-fix-g++-linking.patch || true) && cd - # revert patch

build/test_calc_fee: c/tests/test_calc_fee.c $(VALIDATOR_DEPS)
	$(CXX) $(CFLAGS) $(LDFLAGS) -Ibuild -o $@ c/tests/test_calc_fee.c $(ALL_OBJS)

build/test_ripemd160: c/ripemd160/test_ripemd160.c c/ripemd160/ripemd160.h c/ripemd160/memzero.h $(ALL_OBJS)
	$(CXX) $(CFLAGS) $(LDFLAGS) -Ibuild -o $@ c/ripemd160/test_ripemd160.c $(ALL_OBJS)
	riscv64-unknown-elf-run build/test_ripemd160

build/execution_state.o: deps/evmone/lib/evmone/execution_state.cpp
	$(CXX) $(CXXFLAGS) $(LDFLAGS) -c -o $@ $<
build/baseline.o: deps/evmone/lib/evmone/baseline.cpp
	$(CXX) $(CXXFLAGS) $(LDFLAGS) -c -o $@ $<
build/analysis.o: deps/evmone/lib/evmone/analysis.cpp
	$(CXX) $(CXXFLAGS) $(LDFLAGS) -c -o $@ $<
build/execution.o: deps/evmone/lib/evmone/execution.cpp
	$(CXX) $(CXXFLAGS) $(LDFLAGS) -c -o $@ $<
build/instructions.o: deps/evmone/lib/evmone/instructions.cpp
	$(CXX) $(CXXFLAGS) $(LDFLAGS) -c -o $@ $<
build/instruction_metrics.o: deps/evmone/evmc/lib/instructions/instruction_metrics.c
	$(CXX) $(CXXFLAGS) $(LDFLAGS) -c -o $@ $<
build/instruction_names.o: deps/evmone/evmc/lib/instructions/instruction_names.c
	$(CXX) $(CXXFLAGS) $(LDFLAGS) -c -o $@ $<
build/instructions_calls.o: deps/evmone/lib/evmone/instructions_calls.cpp
	$(CXX) $(CXXFLAGS) $(LDFLAGS) -c -o $@ $<
build/evmone.o: deps/evmone/lib/evmone/evmone.cpp
	$(CXX) $(CXXFLAGS) $(LDFLAGS) -c -o $@ $< -DPROJECT_VERSION=\"0.6.0-dev\"

build/keccak.o: deps/ethash/lib/keccak/keccak.c build/keccakf800.o
	$(CC) $(CFLAGS) $(LDFLAGS) -c -o $@ $<
# build/keccakf1600.o: deps/ethash/lib/keccak/keccakf1600.c
# 	$(CC) $(CFLAGS) $(LDFLAGS) -c -o $@ $<
build/keccakf800.o: deps/ethash/lib/keccak/keccakf800.c
	$(CC) $(CFLAGS) $(LDFLAGS)  -c -o $@ $<

build/memzero.o: c/ripemd160/memzero.c
	$(CXX) $(CFLAGS) $(LDFLAGS) -c -o $@ $<
build/ripemd160.o: c/ripemd160/ripemd160.c
	$(CXX) $(CFLAGS) $(LDFLAGS) -c -o $@ $<

build/platform_util.o: deps/mbedtls/library/platform_util.c
	$(CC) $(CFLAGS) $(LDFLAGS) -c -o $@ $<
build/bignum.o: deps/mbedtls/library/bignum.c
	$(CC) $(CFLAGS) $(LDFLAGS) -c -o $@ $<

build/sha256.o: deps/crypto-algorithms/sha256.c
	$(CXX) $(CFLAGS) $(LDFLAGS) -c -o $@ $<

build/secp256k1_data_info.h: build/dump_secp256k1_data
	$<
build/dump_secp256k1_data: c/dump_secp256k1_data.c $(SECP256K1_SRC)
	mkdir -p build
	gcc $(CFLAGS) -o $@ $<
$(SECP256K1_SRC):
	cd $(SECP_DIR) && \
    (git apply -R workaround-fix-g++-linking.patch || true) && \
		./autogen.sh && \
		CC=$(CC) LD=$(LD) ./configure --with-bignum=no --enable-ecmult-static-precomputation --enable-endomorphism --enable-module-recovery --host=$(TARGET) && \
    make src/ecmult_static_pre_context.h src/ecmult_static_context.h

generate-protocol: check-moleculec-version build/blockchain.h build/godwoken.h
check-moleculec-version:
	test "$$(${MOLC} --version | awk '{ print $$2 }' | tr -d ' ')" = ${MOLC_VERSION}

build/blockchain.mol:
	mkdir -p build
	curl -L -o $@ ${PROTOCOL_SCHEMA_URL}/blockchain.mol

build/godwoken.mol:
	mkdir -p build
	curl -L -o $@ ${PROTOCOL_SCHEMA_URL}/godwoken.mol

build/blockchain.h: build/blockchain.mol
	${MOLC} --language c --schema-file $< > $@

build/godwoken.h: build/godwoken.mol
	${MOLC} --language c --schema-file $< > $@

contract/sudt-erc20-proxy:
	docker run --rm -v $$(pwd)/solidity/erc20:/contracts ethereum/solc:0.8.7 -o /contracts --bin --overwrite /contracts/SudtERC20Proxy_UserDefinedDecimals.sol
	ERC20BIN_SHASUM="$$(ckb-cli util blake2b --binary-path solidity/erc20/ERC20.bin 2>&1 | head -n1)" && \
	echo $$ERC20BIN_SHASUM && \
	if [ "$$ERC20BIN_SHASUM" = "0x4ae01d53e33a739ad261979f54efb066982b7d10a54166a245467756b85daa16" ]; \
	then echo "ERC20BIN_SHASUM matches" ; \
	else echo "ERC20BIN_SHASUM does not match" ; exit 1 ; fi
# ERC20BIN_SHASUM="$$(shasum -a 256 solidity/erc20/ERC20.bin | cut -d' ' -f1)" && \
# if [ "$$ERC20BIN_SHASUM" = "9f7bf1ab25b377ddc339e6de79a800d4c7dc83de7e12057a0129b467794ce3a3" ] ; \
# then echo "ERC20BIN_SHASUM matches" ; \
# else echo "ERC20BIN_SHASUM does not match" ; exit 1 ; fi

# fetch godwoken-scripts from godwoken-prebuilds image,
# including meta-contract and sudt-contract
PREBUILDS := ghcr.io/zeroqn/godwoken-prebuilds:v1.1-feat-change-ckb-decimal-to-18
fetch-gw-scripts:
	mkdir -p build
	docker run --rm -v `pwd`/build:/build-dir \
	   	$(PREBUILDS) \
		cp -r /scripts/godwoken-scripts /build-dir \
		&& echo "Copy godwoken-scripts"

fmt:
	clang-format -i -style=Google c/**/*.*

clean:
	rm -rf build/*
	cd $(SECP_DIR) && [ -f "Makefile" ] && make distclean && make clean || echo "skip secp256k1 clean"
	rm -rf $(SECP256K1_SRC)
