# Generate the contract code hash of SudtERC20Proxy_UserDefinedDecimals
ContractCodeHex=(`cat ./SudtERC20Proxy_UserDefinedDecimals.ContractCode.hex`)
ckb-cli util blake2b --binary-hex $ContractCodeHex

# Result
# 0xde4542f5a5bd32c09cd98e9752281f88900a059aab7ac103edd9df214f136c52
