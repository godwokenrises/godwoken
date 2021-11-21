# Bridge

We offered the CLI tool `gw-tools` to do the layer1 <-> layer2 bridge.

There are also third-party UI tools such as [yokaiswap](https://testnet.yokaiswap.com/bridge/deposit) can do bridge a lot easier, but will not include in this document.

## Limitation

In Godwoken internal, CKB cell's lock field is used to express the custodian of bridged(deposit) assets.
Due to CKB's design, cells must holds enough CKB to cover the storage cost of the lock field, so we require a minimal bridged CKB.

| Bridge direction | CKB only | CKB with Simple UDT |
| ---------------- | -------- | ------------------- |
| Deposit (L1 -> L2) | 290 CKB | 371 CKB |
| Withdraw (L2 -> L1) | 265 CKB | 346 CKB |

> Be careful, if you send a deposit or withdrawal with CKB below the requirement, the deposit or withdrawal may be ignored by Godwoken Sequencer.

### Using GW-tools to Deposit

Use `--help` to view the available commands.

```shell
cargo run --bin gw-tools -- deposit-ckb --help
```

<details>
<summary>Click to view detailed output</summary>

```
gw-tools-deposit-ckb
Deposit CKB to godwoken

USAGE:
	 gw-tools deposit-ckb [OPTIONS] --capacity <capacity> --config-path <config-path>  --privkey-path <privkey-path>  --scripts-deployment-path <scripts-deployment-path>

FLAGS:
	 -h, --help				Prints help informaiton
	 -V, --version				Prints version information

OPTIONS:
	 -c, --capacity <capacity>		CKB capacity to deposit
	     --ckb-rpc <ckb-rpc-url>		CKB jsonrpc rpc sever URL [default: http://127.0.0.1:8114]

	 -o, --config-path <config-path>	The config.homl file path
	 -e, --eth-address <eth-address>	Target eth address, calculated by private key in default
	 -f, --fee <fee>			Transaction fee, default to 0.0001 CKB [default: 0.0001]
	 -g, --godwoken-rpc-url <godwoken-rpc-url>
	 					Godwoken jsonrpc rpc sever URL [default: http://127.0.0.1:8119]

	 -k, --privkey-path <privkey-path>	The private key file path
	     --scripts-deployment-path <scripts-deployment-path>	
	 					The scripts deployment results json file path

```
</details>

### <code>gw-tool deposit</code> Subcommands

|command|description|
|---|---|
|capacity          |The amount of ckb to deposit, the unit is ckb|
|ckb-rpc           |ckb node URL, defaults to http://127.0.0.1:8114/|
|config-path          |The config.toml file required for godwoken to run|
|eth-address          |Target eth address to deposit|
|fee          |The transaction fee, this is a ckb transaction and the default rate is 0.0001 ckb|
|godwoken-rpc-url          |The RPC address of Godwoken, by default http://127.0.0.1:8119/|
|privkey-path          |A file written with the private key (hex string) which is used to pay the deposit fee|
|scripts-deployment-path          |json file path of the [script's deployment results](https://github.com/nervosnetwork/godwoken-public/blob/master/testnet/config/scripts-deploy-result.json)|

---

### Using GW-tools to Withdraw

Use `--help` to view the available commands.

```shell
cargo run --bin gw-tools -- withdraw --help
```

<details>
<summary>Click to view detailed output</summary>


```
gw-tools-withdraw
withdraw CKB / sUDT from godwoken

USAGE:
	 gw-tools withdraw [OPTIONS] --capacity <capacity> --config-path <config-path> --owner-ckb-address <owner-ckb-address> --privkey-path <privkey-path>  --scripts-deployment-path <scripts-deployment-path>

FLAGS:
	 -h, --help				Prints help informaiton
	 -V, --version				Prints version information

OPTIONS:
	 -m, --amount <amount>		 sUDT amount to withdrawal [default: 0]
	 -c, --capacity <capacity>		CKB capacity to withdrawal
	 -o, --config-path <config-path>	The config.homl file path
	 -g, --godwoken-rpc-url <godwoken-rpc-url>
	 					Godwoken jsonrpc rpc sever URL [default: http://127.0.0.1:8119]

	 -a, --owner-ckb-address <owner-ckb-address>	owner ckb address (to)
	 -k, --privkey-path <privkey-path>	The private key file path
	     --scripts-deployment-path <scripts-deployment-path>	
	 					The scripts deployment results json file path

	 	 -sudt-script-hash <sudt-script-hash>	l1 sudt script hash, default for withdrawal CKB [default: 0x0000000000000000000000000000000000000000000000000000000000000000]

```
</details>

### <code>gw-tool withdraw</code> Subcommands

|command|description|
|---|---|
|amount             |The amount of sUDT|
|capacity             |The amount of ckb to withdraw, the unit is ckb|
|config-path             |The config.toml file required for godwoken to run|
|godwoken-rpc-url             |The RPC address of Godwoken, by default http://127.0.0.1:8119/|
|owner-ckb-address             |ckb address of the recipient|
|privkey-path             |A file written with the private key (hex string) which is used to pay the deposit fee|
|scripts-deployment-path             |json file path of the [script's deployment results](https://github.com/nervosnetwork/godwoken-public/blob/master/testnet/config/scripts-deploy-result.json)|
|sudt-script-hash             |The script hash of sudt on Layer 1, defaults to 0x0000000000000000000000000000000000000000000000000000, indicating only ckb is redeemed (amount left unfilled or filled with 0)|
