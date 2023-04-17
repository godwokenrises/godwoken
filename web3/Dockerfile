FROM node:18-bullseye

COPY ./web3 /godwoken-web3

RUN cd /godwoken-web3 && yarn && yarn build

# pm2 is no longer used.
# RUN npm install pm2 -g
# TODO: remove these lines in the next release

RUN apt-get update \
 && apt-get dist-upgrade -y \
 && apt-get install curl -y \
 && apt-get install jq -y \
 && apt-get clean \
 && echo "Finished installing dependencies"

EXPOSE 8024 3000
