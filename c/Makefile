TARGET := riscv64-unknown-linux-gnu
CC := $(TARGET)-gcc
LD := $(TARGET)-gcc
OBJCOPY := $(TARGET)-objcopy
SECP256K1_INCLUDES := -I deps/ckb-miscellaneous-scripts/deps -I deps/ckb-miscellaneous-scripts/build/ -I deps/ckb-miscellaneous-scripts/deps/secp256k1/src/ -I deps/ckb-miscellaneous-scripts/deps/secp256k1/
CINCLUDES := -I deps/ckb-c-stdlib -I deps/ckb-c-stdlib/libc -I deps/molecule -I . -I build
CFLAGS := $(CINCLUDES) $(SECP256K1_INCLUDES) -DCKB_C_STDLIB_PRINTF -fPIC -O3 -nostdinc -nostdlib -nostartfiles -fvisibility=hidden -Wall -Werror -Wno-nonnull -Wno-nonnull-compare -Wno-unused-function -g
LDFLAGS := -Wl,-static -fdata-sections -ffunction-sections -Wl,--gc-sections
MOLC := moleculec
MOLC_VERSION := 0.6.1
PROTOCOL_SCHEMA_DIR := ../crates/types/schemas

# docker pull nervos/ckb-riscv-gnu-toolchain:gnu-bionic-20191012
BUILDER_DOCKER := nervos/ckb-riscv-gnu-toolchain@sha256:aae8a3f79705f67d505d1f1d5ddc694a4fd537ed1c7e9622420a470d59ba2ec3

GENERATORS := build/meta-contract-generator build/sudt-generator build/examples/sum-generator
VALIDATORS := build/meta-contract-validator build/sudt-validator build/examples/sum-validator
ACCOUNT_LOCKS := build/account_locks/eth-account-lock
SECP256K1_HELPER := deps/ckb-miscellaneous-scripts/build/secp256k1_data_info.h

BINS := $(GENERATORS) $(VALIDATORS) $(ACCOUNT_LOCKS)

all-via-docker: generate-protocol $(SECP256K1_HELPER)
	docker run --rm -v `pwd`:/code -w /code ${BUILDER_DOCKER} bash -c "make all"

debug-all-via-docker: generate-protocol $(SECP256K1_HELPER)
	docker run --rm -v `pwd`:/code -w /code ${BUILDER_DOCKER} bash -c "make debug-all"

all: $(BINS)

clean:
	rm $(BINS)

debug-all: CFLAGS += -DCKB_C_STDLIB_PRINTF 
debug-all: all

# secp256k1_helper.h
$(SECP256K1_HELPER):
	cd deps/ckb-miscellaneous-scripts && git submodule init && git submodule update -r && make all-via-docker

build/meta-contract-generator: contracts/meta_contract.c gw_def.h generator_utils.h
	$(CC) $(CFLAGS) -DGW_GENERATOR $(LDFLAGS) -o $@ $<
	$(OBJCOPY) --only-keep-debug $@ $@.debug
	$(OBJCOPY) --strip-debug --strip-all $@

build/meta-contract-validator: contracts/meta_contract.c gw_def.h validator_utils.h
	$(CC) $(CFLAGS) -DGW_VALIDATOR $(LDFLAGS) -o $@ $<
	$(OBJCOPY) --only-keep-debug $@ $@.debug
	$(OBJCOPY) --strip-debug --strip-all $@

build/sudt-generator: contracts/sudt.c sudt_utils.h gw_def.h generator_utils.h
	$(CC) $(CFLAGS) -DGW_GENERATOR $(LDFLAGS) -o $@ $<
	$(OBJCOPY) --only-keep-debug $@ $@.debug
	$(OBJCOPY) --strip-debug --strip-all $@

build/sudt-validator: contracts/sudt.c sudt_utils.h gw_def.h validator_utils.h
	$(CC) $(CFLAGS) -DGW_VALIDATOR $(LDFLAGS) -o $@ $<
	$(OBJCOPY) --only-keep-debug $@ $@.debug
	$(OBJCOPY) --strip-debug --strip-all $@

build/account_locks/eth-account-lock: account_locks/eth_account_lock.c
	$(CC) $(CFLAGS) $(LDFLAGS) -o $@ $<
	$(OBJCOPY) --only-keep-debug $@ $@.debug
	$(OBJCOPY) --strip-debug --strip-all $@

build/examples/sum-generator: examples/sum.c gw_def.h generator_utils.h
	$(CC) $(CFLAGS) -DGW_GENERATOR $(LDFLAGS) -o $@ $<
	$(OBJCOPY) --only-keep-debug $@ $@.debug
	$(OBJCOPY) --strip-debug --strip-all $@

build/examples/sum-validator: examples/sum.c gw_def.h validator_utils.h
	$(CC) $(CFLAGS) -DGW_VALIDATOR $(LDFLAGS) -o $@ $<
	$(OBJCOPY) --only-keep-debug $@ $@.debug
	$(OBJCOPY) --strip-debug --strip-all $@
 
generate-protocol: check-moleculec-version build/blockchain.h build/godwoken.h

fmt:
	clang-format -i -style=Google $(wildcard ./*.h ./*.c)
	git diff --exit-code $(wildcard ./*.h ./*.c)

check-moleculec-version:
	test "$$(${MOLC} --version | awk '{ print $$2 }' | tr -d ' ')" = ${MOLC_VERSION}

build/blockchain.h: ${PROTOCOL_SCHEMA_DIR}/blockchain.mol
	${MOLC} --language c --schema-file $< > $@

build/godwoken.h: ${PROTOCOL_SCHEMA_DIR}/godwoken.mol
	${MOLC} --language c --schema-file $< > $@

install-tools:
	if [ ! -x "$$(command -v "${MOLC}")" ] \
			|| [ "$$(${MOLC} --version | awk '{ print $$2 }' | tr -d ' ')" != "${MOLC_VERSION}" ]; then \
		cargo install --force --version "${MOLC_VERSION}" "${MOLC}"; \
	fi

