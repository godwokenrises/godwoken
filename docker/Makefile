SHELL := /bin/bash

# components repos
OMNI_LOCK_REPO := https://github.com/nervosnetwork/ckb-production-scripts.git

# components tags
OMNI_LOCK_REF := rc_lock

define prepare_repo
	if [ ! -d "build/$(3)" ]; then\
		git clone --depth=1 $(1) build/$(3);\
	fi
	cd build/$(3);\
	git fetch origin $(2);\
	git checkout FETCH_HEAD;\
	git submodule update --init --recursive --depth=1;\
	echo "$(3)-sha1=$$(git rev-parse HEAD)" >> ../versions
endef

prepare-repos:
	mkdir -p build
	echo "godwoken-sha1=$$(git rev-parse HEAD)" >> build/versions
	echo "GODWOKEN_REF=$$(git describe --tags --exact-match 2> /dev/null || git symbolic-ref -q --short HEAD || git rev-parse --short HEAD) $$(git rev-parse --short HEAD)" >> build/versions
	$(call prepare_repo,$(OMNI_LOCK_REPO),$(OMNI_LOCK_REF),ckb-production-scripts)
	echo "OMNI_LOCK_REF=$(OMNI_LOCK_REF) $$(cd build/ckb-production-scripts && git rev-parse --short HEAD)" >> build/versions

build-components: prepare-repos
	cd build/ckb-production-scripts && make all-via-docker
	cd ../gwos-evm && git submodule update --init --recursive --depth=1 && make dist && cd -
	cd ../gwos && cd c && make && cd .. && capsule build --release --debug-output && cd ../..
	cd ../ && rustup component add rustfmt && RUSTFLAGS="-C target-cpu=native" CARGO_PROFILE_RELEASE_LTO=true cargo build --release

build-push:
	make build-components
	@read -p "Please Enter New Image Tag: " VERSION ; \
	docker build . -t nervos/godwoken-prebuilds:$$VERSION ; \
	docker push nervos/godwoken-prebuilds:$$VERSION

test:
	make build-components
	docker build . -t godwokenrises/godwoken-prebuilds:latest-test
	mkdir -p `pwd`/test-result/scripts
	mkdir -p `pwd`/test-result/bin
	docker run -it -d --name dummy godwokenrises/godwoken-prebuilds:latest-test
	docker cp dummy:/scripts/. `pwd`/test-result/scripts
	docker cp dummy:/bin/godwoken `pwd`/test-result/bin
	docker cp dummy:/bin/gw-tools `pwd`/test-result/bin
	docker rm -f dummy
	make test-files

test-files:
	echo "start checking build result..."
# compare scripts files
	make test-scripts-files
	make test-polyjuice-files
# compare bin files
	cd `pwd`/test-result/bin && ./godwoken --version && ./gw-tools --version
	[ -e "test-result" ] && rm -rf test-result

test-scripts-files:
	source tool.sh && check_scripts_files_exists

test-polyjuice-files:
	source tool.sh && check_polyjuice_files_exists 
