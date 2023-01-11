# Historical versions refer to checksum.txt
# https://github.com/godwokenrises/godwoken/pkgs/container/godwoken/62869296?tag=v1.7.4
FROM ghcr.io/godwokenrises/godwoken:v1.7.4 as historical-versions

################################################################################

# https://hub.docker.com/_/ubuntu/
FROM ubuntu:focal
LABEL description="Docker image containing all binaries used by Godwoken, saving you the hassles of building them yourself."
LABEL maintainer="Godwoken Core Dev"

RUN mkdir -p /scripts/godwoken-scripts \
 && mkdir -p /scripts/godwoken-polyjuice \
 && mkdir /ckb 

RUN apt-get update \
 && apt-get dist-upgrade -y \
 && apt-get install -y curl jq \
 && apt-get clean \
 && echo 'Finished installing OS updates'

# ckb
RUN cd /ckb \
 && curl -LO https://github.com/nervosnetwork/ckb/releases/download/v0.103.0/ckb_v0.103.0_x86_64-unknown-linux-gnu.tar.gz \
 && tar xzf ckb_v0.103.0_x86_64-unknown-linux-gnu.tar.gz \
 && cp ckb_v0.103.0_x86_64-unknown-linux-gnu/ckb /bin/ckb \
 && cp ckb_v0.103.0_x86_64-unknown-linux-gnu/ckb-cli /bin/ckb-cli \
 && rm -rf /ckb

# Copy historical versions (refer to checksum.txt)
#
# If <dest> doesn’t exist, it is created along with all missing directories in its path.
# refer to https://docs.docker.com/engine/reference/builder/#copy
COPY docker/checksum.txt /scripts/
COPY --from=historical-versions /scripts/godwoken-polyjuice-v1.1.5-beta/ \
                                /scripts/godwoken-polyjuice-v1.1.5-beta/
COPY --from=historical-versions /scripts/godwoken-polyjuice-v1.2.0/ \
                                /scripts/godwoken-polyjuice-v1.2.0/
COPY --from=historical-versions /scripts/godwoken-polyjuice-v1.4.0/ \
                                /scripts/godwoken-polyjuice-v1.4.0/
COPY --from=historical-versions /scripts/godwoken-polyjuice-v1.4.1/ \
                                /scripts/godwoken-polyjuice-v1.4.1/
COPY --from=historical-versions /scripts/godwoken-polyjuice-v1.4.2/ \
                                /scripts/godwoken-polyjuice-v1.4.2/
COPY --from=historical-versions /scripts/godwoken-polyjuice-v1.4.4/ \
                                /scripts/godwoken-polyjuice-v1.4.4/
COPY --from=historical-versions /scripts/godwoken-polyjuice-v1.4.5/ \
                                /scripts/godwoken-polyjuice-v1.4.5/
COPY --from=historical-versions /scripts/godwoken-polyjuice-v1.5.0/ \
                                /scripts/godwoken-polyjuice-v1.5.0/
COPY --from=historical-versions /scripts/godwoken-polyjuice/* \
                                /scripts/godwoken-polyjuice-v1.5.1/

#################################### latest ####################################
# COPY [--chown=<user>:<group>] ["<src>",... "<dest>"]

# /scripts/godwoken-polyjuice
COPY gwos-evm/build/*generator* \
     gwos-evm/build/*validator* \
     /scripts/godwoken-polyjuice/
# TODO: remove *.aot in Polyjuice Makefile
# RUN find /scripts -type f -name '*.aot' -exec rm {} \;

# /scripts/omni-lock and /scripts/godwoken-scripts
COPY docker/build/ckb-production-scripts/build/omni_lock \
     gwos/build/release/* \
     gwos/c/build/*-generator \ 
     gwos/c/build/*-validator \
     gwos/c/build/account_locks/* \
     /scripts/godwoken-scripts/

# godwoken
COPY target/release/godwoken \
     target/release/gw-tools \
     docker/gw-healthcheck.sh \
     /bin/
################################################################################


WORKDIR /deploy

CMD [ "godwoken", "--version" ]
