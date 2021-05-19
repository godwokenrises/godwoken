# `pwd` => ./tests
# start your godwoken dev chain, and then update your godwoken configs into `tests/configs`
rm -r indexer-data
export SCRIPT_DEPLOY_RESULT_PATH=${PWD}/configs/scripts-deploy-result.json
export GODWOKEN_CONFIG_PATH=${PWD}/configs/godwoken-config.toml

# clone godwoken-examples
git clone --depth 1 --branch pkg https://github.com/Flouse/godwoken-examples examples

# use node@14
cd examples && yarn && yarn build-all 
# copy and convert config format
yarn copy-configs
yarn convert-config-format # convert `godwoken-config.toml` to `godwoken-config.json`
yarn build-all
npx pkg -t node14-macos,node14-linux packages/tools/lib/account-cli.js
mv account-cli* ../

###
cd .. && rm -rf examples
# deposit using account-cli.js
# start_seconds=`date +%s`
# LUMOS_CONFIG_FILE=configs/lumos-config.json \
# 	node ./godwoken-examples/packages/tools/lib/account-cli.js \
# 		--godwoken-rpc http://192.168.5.102:8119 \
# 		deposit \
# 		--rpc http://192.168.5.102:8114 \
# 		-p 0xdd50cac37ec6dd12539a968c1a2cbedda75bd8724f7bcad486548eaabb87fc8b \
# 		-c 30000000000
# end_seconds=`date +%s`
# echo Total elapsed time: $((end_seconds-start_seconds)) seconds.

# deposit using account-cli-macos executalbe
# start_seconds=`date +%s`
# LUMOS_CONFIG_FILE=configs/lumos-config.json \
# 	./godwoken-examples/account-cli-macos \
# 		--godwoken-rpc http://192.168.5.102:8119 \
# 		deposit \
# 		--rpc http://192.168.5.102:8114 \
# 		-p 0xdd50cac37ec6dd12539a968c1a2cbedda75bd8724f7bcad486548eaabb87fc8b \
# 		-c 30000000000
# end_seconds=`date +%s`
# echo Total elapsed time: $((end_seconds-start_seconds)) seconds.
