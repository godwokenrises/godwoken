use std::process::{Command};
use crate::{Spec};
use std::env;
//TODO: https://docs.rs/env_logger/0.8.3/env_logger/ 
//TODO: Redirect both stdout and stderr of child process to the same file

pub struct SimpleCase;

impl Spec for SimpleCase {
	/// Case: 
	/// 	1. deposit CKB from layer1 to layer2
	///		2. godwoken transfer from MINER to USER1
	///   3. withdraw CKB from layer2 to layer1
	fn run(&self) {
		let CKB_RPC: String = env::var("CKB_RPC")
			.unwrap_or("http://127.0.0.1:8114".to_string());
		let MINER_PRIVATE_KEY: String = env::var("MINER_PRIVATE_KEY")
			.unwrap_or("0xdd50cac37ec6dd12539a968c1a2cbedda75bd8724f7bcad486548eaabb87fc8b".to_string());
		let USER1_PRIVATE_KEY: String = env::var("USER1_PRIVATE_KEY")
			.unwrap_or("0x6cd5e7be2f6504aa5ae7c0c04178d8f47b7cfc63b71d95d9e6282f5b090431bf".to_string());
		// println!("MINER_PRIVATE_KEY: {}", MINER_PRIVATE_KEY);

		// call account-cli
		println!("\nSimpleCase: call account-cli to deposit -> transfer -> withdraw");
		let _exit_status: std::process::ExitStatus = account_cli()
			.arg("deposit")
			.args(&["--rpc", &CKB_RPC])
			.args(&["-p", &MINER_PRIVATE_KEY])
			.args(&["-c", "60000000000"]) // 600 CKBytes = 60,000,000,000 Shannons
			.status()
			.expect("failed to deposit CKB from layer1 to layer2");
		//TODO: if _exit_status.status.success()

		// TODO: get account_id
		// query balance
		// TODO: process the output to balance values
		println!("\nAccount ID: 2");
		let mut _get_balance_status = account_cli()
		  .args(&["get-balance", "2"])
			.status()
			.expect("failed to get-balance");
		println!("\nAccount ID: 3");
		_get_balance_status = account_cli()
		  .args(&["get-balance", "3"])
			.status()
			.expect("failed to get-balance");

		// transfer
		println!("\nTransfer 10001 Shannons from ID:2 to ID:3");
		let _transfer_status = account_cli()
			.arg("transfer")
			.args(&["--rpc", &CKB_RPC])
			.args(&["-p", &MINER_PRIVATE_KEY])
			.args(&["--amount", "10000000001"])
			.args(&["--fee", "100"])
  		.args(&["--to-id", "3"])
			.args(&["--sudt-id", "1"])
			.status()
			.expect("failed to transfer");

		// withdraw
		println!("\nAccount ID: 2 withdraw 40000000000 shannons CKB from godwoken");
		let mut _withdrawal_status = account_cli().arg("withdraw")
			.args(&["--rpc", &CKB_RPC])
		  .args(&["-p", &MINER_PRIVATE_KEY])
			.args(&["--owner-ckb-address", "ckt1qyqy84gfm9ljvqr69p0njfqullx5zy2hr9kq0pd3n5"])
			.args(&["--capacity", "40000000000"]) // 40,000,000,000 Shannons = 400 CKBytes
			.status();
		println!("\nAccount ID: 3 withdraw 10000 shannons CKB from godwoken");
			_withdrawal_status = account_cli().arg("withdraw")
			.args(&["--rpc", &CKB_RPC])
		  .args(&["-p", &USER1_PRIVATE_KEY])
			.args(&["--owner-ckb-address", "ckt1qyqf22qfzaer95xm5d2m5km0f6k288x9warqnhsf4m"])
			.args(&["--capacity", "10000000000"])
			.status();

		// query balance after confirm
		println!("\nAccount ID: 2");
		_get_balance_status = account_cli()
		  .args(&["get-balance", "2"])
			.status()
			.expect("failed to get-balance");
		println!("\nAccount ID: 3");
		_get_balance_status = account_cli()
		  .args(&["get-balance", "3"])
			.status()
			.expect("failed to get-balance");

			// TODO: assert_eq!
			// id2 += 20000000000 - 100000001
			// id3 += 1
	}
}

fn account_cli() -> Command {
	let mut account_cli = if cfg!(target_os = "linux") {
		Command::new("./account-cli-linux")
	} else if cfg!(target_os = "macos"){
		Command::new("./account-cli-macos")
	} else {
		panic!("windows is not supported yet.");
	};
	let GODWOKEN_RPC: String = env::var("GODWOKEN_RPC")
			.unwrap_or("http://127.0.0.1:8119".to_string());
	account_cli
		.env("LUMOS_CONFIG_FILE", "configs/lumos-config.json")
		.args(&["--godwoken-rpc", &GODWOKEN_RPC]);
	account_cli
}
