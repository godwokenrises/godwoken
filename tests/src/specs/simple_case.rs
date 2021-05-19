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
		let _exit_status: std::process::ExitStatus = Command::new("./godwoken-examples/account-cli-macos")
			.env("LUMOS_CONFIG_FILE", "configs/lumos-config.json")
			.args(&["--godwoken-rpc", "http://192.168.5.102:8119"])
			.arg("deposit")
			.args(&["--rpc", "http://192.168.5.102:8114"])
			.args(&["-p", "0xdd50cac37ec6dd12539a968c1a2cbedda75bd8724f7bcad486548eaabb87fc8b"])
			.args(&["-c", "30000000000"])
			.status()
			.expect("failed to execute process in Linux/macOS");
	}
}
