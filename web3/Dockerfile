FROM node:18-bullseye

COPY ./web3 /godwoken-web3

RUN apt-get update \
 && apt-get dist-upgrade -y \
 && apt-get install curl -y \
 && apt-get install jq -y \
 && rm -rf /var/lib/apt/lists/* \
 && echo "Finished installing dependencies"
RUN cd /godwoken-web3 && yarn && yarn build

EXPOSE 8024 3000
