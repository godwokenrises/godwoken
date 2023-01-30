FROM rust:1-bullseye as builder

RUN apt-get update
RUN apt-get -y install --no-install-recommends llvm-dev clang libclang-dev libssl-dev

COPY . /godwoken
WORKDIR /godwoken/web3
RUN rustup component add rustfmt && cargo build --release

FROM ubuntu:focal

RUN apt-get update \
 && apt-get dist-upgrade -y \
 && apt-get install -y openssl \
 && apt-get install -y libcurl4 \
 && apt-get clean \
 && echo 'Finished installing OS updates'

# godwoken-web3 indexer
COPY --from=builder /godwoken/web3/target/release/gw-web3-indexer /bin/gw-web3-indexer

RUN mkdir -p /web3
WORKDIR /web3

CMD [ "gw-web3-indexer", "--version" ]
