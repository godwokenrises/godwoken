SHELL := /bin/bash

IMAGE_NAME := nervos/godwoken-web3-prebuilds
INDEXER_IMAGE_NAME := nervos/godwoken-web3-indexer-prebuilds

build-push:
	@read -p "Please Enter New Image Tag: " VERSION ; \
	docker build . -t ${IMAGE_NAME}:$$VERSION ; \
	docker push ${IMAGE_NAME}:$$VERSION

build-test-image:
	docker build . -t ${IMAGE_NAME}:latest-test

build-indexer-image:
	@read -p "Please Enter New Indexer Image Tag: " VERSION ; \
	docker build -f ./docker/indexer/Dockerfile . -t ${INDEXER_IMAGE_NAME}:$$VERSION ; \

test:
	make build-test-image
	make test-jq
	make test-web3

test-jq:
	docker run --rm ${IMAGE_NAME}:latest-test /bin/bash -c "jq -V"

test-web3:
	docker run --rm -v `pwd`:/app ${IMAGE_NAME}:latest-test /bin/bash -c "cp -r godwoken-web3/node_modules app/node_modules"
	yarn check --verify-tree
	
migrate:
	yarn run migrate:latest
