use std::process::{Command};
use crate::{Spec};

pub struct SimpleCase;

impl Spec for SimpleCase {
	/// Case: 
	/// 	1. deposit CKB from layer1 to layer2
	///		2...
	fn run(&self) {
		// call account-cli
		println!("\ncall account-cli to deposit -> transfer -> withdraw");
		let _exit_status: std::process::ExitStatus = account_cli()
			.arg("deposit")
			.args(&["--rpc", "http://192.168.5.102:8114"])
			.args(&["-p", "0x6cd5e7be2f6504aa5ae7c0c04178d8f47b7cfc63b71d95d9e6282f5b090431bf"])
			.args(&["-c", "30000000000"])
			.status()
			.expect("failed to deposit CKB from layer1 to layer2");
		
		// TODO: get account_id
		// query balance
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
		println!("\nTransfer 10001 CKB from ID:2 to ID:3");
		let _transfer_status = account_cli()
			.arg("transfer")
			.args(&["--rpc", "http://192.168.5.102:8114"])
			.args(&["-p", "0xdd50cac37ec6dd12539a968c1a2cbedda75bd8724f7bcad486548eaabb87fc8b"])
			.args(&["--amount", "10001"])
			.args(&["--fee", "100"])
  		.args(&["--to-id", "3"])
			.args(&["--sudt-id", "1"])
			.status()
			.expect("failed to transfer");

		// withdraw
		println!("\nAccount ID: 2 withdraw 40000000000 shannons CKB from godwoken");
		let _withdrawal_status = account_cli().arg("withdraw")
			.args(&["--rpc", "http://192.168.5.102:8114"])
		  .args(&["-p", "0xdd50cac37ec6dd12539a968c1a2cbedda75bd8724f7bcad486548eaabb87fc8b"])
			.args(&["--owner-ckb-address", "ckt1qyqy84gfm9ljvqr69p0njfqullx5zy2hr9kq0pd3n5"])
			.args(&["--capacity", "40000000000"])
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
	account_cli
		.env("LUMOS_CONFIG_FILE", "configs/lumos-config.json")
		.args(&["--godwoken-rpc", "http://192.168.5.102:8119"]);
	account_cli
}
