# deposit 
start_seconds=`date +%s`
LUMOS_CONFIG_FILE=configs/lumos-config.json \
	./account-cli \
		--godwoken-rpc http://192.168.5.102:8119 \
		deposit \
		--rpc http://192.168.5.102:8114 \
		-p 0xdd50cac37ec6dd12539a968c1a2cbedda75bd8724f7bcad486548eaabb87fc8b \
		-c 30000000000
end_seconds=`date +%s`
echo Total elapsed time: $((end_seconds-start_seconds)) seconds.
