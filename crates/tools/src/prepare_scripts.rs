use crate::deploy_scripts::Programs;
use anyhow::Result;
use ckb_fixed_hash::H256;
use ckb_jsonrpc_types::{JsonBytes, Script, ScriptHashType};
use clap::arg_enum;
use serde::{Deserialize, Serialize};
use std::{
    env,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use url::Url;

const GODWOKEN_SCRIPTS: &str = "godwoken-scripts";
const GODWOKEN_POLYJUICE: &str = "godwoken-polyjuice";
const CLERKB: &str = "clerkb";

arg_enum! {
    #[derive(Debug)]
    pub enum ScriptsBuildMode {
        Build,
        Copy
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug)]
struct ScriptsBuildConfig {
    repos: Repos,
    prebuild_image: PathBuf,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug)]
struct Repos {
    godwoken_scripts: Url,
    godwoken_polyjuice: Url,
    clerkb: Url,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug)]
struct BuildScriptsResult {
    programs: Programs,
    lock: Script,
    build_scripts: BuildScripts,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug)]
struct BuildScripts {
    // scripts form godwoken-scripts
    always_success: PathBuf,
    custodian_lock: PathBuf,
    deposition_lock: PathBuf,
    withdrawal_lock: PathBuf,
    challenge_lock: PathBuf,
    stake_lock: PathBuf,
    state_validator: PathBuf,
    l2_sudt_validator: PathBuf,
    eth_account_lock: PathBuf,
    // tron_account_lock: PathBuf,
    meta_contract_validator: PathBuf,
    meta_contract_generator: PathBuf,

    // scripts from godwoken-Polyjuice
    polyjuice_validator: PathBuf,
    polyjuice_generator: PathBuf,

    // scripts from clerkb
    state_validator_lock: PathBuf,
    poa_state: PathBuf,
}

pub fn prepare_scripts(
    mode: ScriptsBuildMode,
    input_path: &Path,
    repos_dir: &Path,
    scripts_dir: &Path,
    output_path: &Path,
) -> Result<()> {
    let input = fs::read_to_string(input_path)?;
    let scripts_build_config: ScriptsBuildConfig = serde_json::from_str(input.as_str())?;
    match mode {
        ScriptsBuildMode::Build => {
            prepare_scripts_in_build_mode(scripts_build_config.repos, repos_dir, scripts_dir)?
        }
        ScriptsBuildMode::Copy => {
            prepare_scripts_in_copy_mode(&scripts_build_config.prebuild_image, scripts_dir)
        }
    }
    let build_scripts = check_scripts_build_result(scripts_dir);
    generate_script_deploy_config(build_scripts, output_path)
}

fn prepare_scripts_in_build_mode(repos: Repos, repos_dir: &Path, scripts_dir: &Path) -> Result<()> {
    run_pull_code(repos.godwoken_scripts, true, repos_dir, GODWOKEN_SCRIPTS)?;
    run_pull_code(
        repos.godwoken_polyjuice,
        true,
        repos_dir,
        GODWOKEN_POLYJUICE,
    )?;
    run_pull_code(repos.clerkb, true, repos_dir, CLERKB)?;
    build_godwoken_scripts(repos_dir, GODWOKEN_SCRIPTS);
    build_godwoken_polyjuice(repos_dir, GODWOKEN_POLYJUICE);
    build_clerkb(repos_dir, CLERKB);
    copy_scripts_to_target(repos_dir, scripts_dir)?;
    Ok(())
}

fn prepare_scripts_in_copy_mode(prebuild_image: &PathBuf, scripts_dir: &Path) {
    let current_dir = env::current_dir().expect("get working dir");
    let target_dir = make_target_dir(&current_dir, &scripts_dir.display().to_string());
    let temp_dir_in_container = "temp";
    let volumn_bind = format!("-v{}:/{}", target_dir, temp_dir_in_container);
    run_command(
        "docker",
        vec![
            "run",
            "--rm",
            &volumn_bind,
            &prebuild_image.display().to_string(),
            "cp",
            "-r",
            "scripts/.",
            temp_dir_in_container,
        ],
    )
    .expect("docker run cp scripts");
}

fn check_scripts_build_result(scripts_dir: &Path) -> BuildScripts {
    // check scripts form godwoken-scripts
    let mut always_success = PathBuf::from(scripts_dir);
    always_success.push(GODWOKEN_SCRIPTS);
    always_success.push("always-success");
    assert!(always_success.exists());

    let mut custodian_lock = PathBuf::from(scripts_dir);
    custodian_lock.push(GODWOKEN_SCRIPTS);
    custodian_lock.push("custodian-lock");
    assert!(custodian_lock.exists());

    let mut deposition_lock = PathBuf::from(scripts_dir);
    deposition_lock.push(GODWOKEN_SCRIPTS);
    deposition_lock.push("deposition-lock");
    assert!(deposition_lock.exists());

    let mut withdrawal_lock = PathBuf::from(scripts_dir);
    withdrawal_lock.push(GODWOKEN_SCRIPTS);
    withdrawal_lock.push("withdrawal-lock");
    assert!(withdrawal_lock.exists());

    let mut challenge_lock = PathBuf::from(scripts_dir);
    challenge_lock.push(GODWOKEN_SCRIPTS);
    challenge_lock.push("challenge-lock");
    assert!(challenge_lock.exists());

    let mut stake_lock = PathBuf::from(scripts_dir);
    stake_lock.push(GODWOKEN_SCRIPTS);
    stake_lock.push("stake-lock");
    assert!(stake_lock.exists());

    let mut state_validator = PathBuf::from(scripts_dir);
    state_validator.push(GODWOKEN_SCRIPTS);
    state_validator.push("state-validator");
    assert!(state_validator.exists());

    let mut l2_sudt_validator = PathBuf::from(scripts_dir);
    l2_sudt_validator.push(GODWOKEN_SCRIPTS);
    l2_sudt_validator.push("sudt-validator");
    assert!(l2_sudt_validator.exists());

    let mut eth_account_lock = PathBuf::from(scripts_dir);
    eth_account_lock.push(GODWOKEN_SCRIPTS);
    eth_account_lock.push("eth-account-lock");
    assert!(eth_account_lock.exists());

    // let mut tron_account_lock = PathBuf::from(scripts_dir);
    // tron_account_lock.push(GODWOKEN_SCRIPTS);
    // tron_account_lock.push("tron-account-lock");
    // assert!(tron_account_lock.exists());

    let mut meta_contract_validator = PathBuf::from(scripts_dir);
    meta_contract_validator.push(GODWOKEN_SCRIPTS);
    meta_contract_validator.push("meta-contract-validator");
    assert!(meta_contract_validator.exists());

    let mut meta_contract_generator = PathBuf::from(scripts_dir);
    meta_contract_generator.push(GODWOKEN_SCRIPTS);
    meta_contract_generator.push("meta-contract-generator");
    assert!(meta_contract_generator.exists());

    // check scripts from godwoken-Polyjuice
    let mut polyjuice_validator = PathBuf::from(scripts_dir);
    polyjuice_validator.push(GODWOKEN_POLYJUICE);
    polyjuice_validator.push("validator");
    assert!(polyjuice_validator.exists());

    let mut polyjuice_generator = PathBuf::from(scripts_dir);
    polyjuice_generator.push(GODWOKEN_POLYJUICE);
    polyjuice_generator.push("generator");
    assert!(polyjuice_generator.exists());

    // check scripts from clerkb
    let mut state_validator_lock = PathBuf::from(scripts_dir);
    state_validator_lock.push(CLERKB);
    state_validator_lock.push("poa");
    assert!(state_validator_lock.exists());

    let mut poa_state = PathBuf::from(scripts_dir);
    poa_state.push(CLERKB);
    poa_state.push("state");
    assert!(poa_state.exists());

    BuildScripts {
        // scripts from godwoken-scripts
        always_success,
        custodian_lock,
        deposition_lock,
        withdrawal_lock,
        challenge_lock,
        stake_lock,
        state_validator,
        l2_sudt_validator,
        eth_account_lock,
        // tron_account_lock,
        meta_contract_validator,
        meta_contract_generator,

        // scripts from godwoken-Polyjuice
        polyjuice_validator,
        polyjuice_generator,

        // scripts from clerkb
        state_validator_lock,
        poa_state,
    }
}

fn generate_script_deploy_config(build_scripts: BuildScripts, output_path: &Path) -> Result<()> {
    let programs = Programs {
        custodian_lock: build_scripts.custodian_lock.clone(),
        deposition_lock: build_scripts.deposition_lock.clone(),
        withdrawal_lock: build_scripts.withdrawal_lock.clone(),
        challenge_lock: build_scripts.always_success.clone(), // always_success
        stake_lock: build_scripts.stake_lock.clone(),
        state_validator: build_scripts.always_success.clone(), // always_success
        l2_sudt_validator: build_scripts.always_success.clone(), // always_success
        eth_account_lock: build_scripts.always_success.clone(), // always_success
        tron_account_lock: build_scripts.always_success.clone(), // always_success
        meta_contract_validator: build_scripts.always_success.clone(), // always_success
        polyjuice_validator: build_scripts.always_success.clone(), // always_success
        state_validator_lock: build_scripts.state_validator_lock.clone(),
        poa_state: build_scripts.poa_state.clone(),
    };
    let lock = Script {
        code_hash: H256::default(),
        hash_type: ScriptHashType::Data,
        args: JsonBytes::default(),
    };
    let build_scripts_result = BuildScriptsResult {
        programs,
        lock,
        build_scripts,
    };
    let output_content =
        serde_json::to_string_pretty(&build_scripts_result).expect("serde json to string pretty");
    let output_dir = output_path.parent().expect("get output dir");
    run_command("mkdir", vec!["-p", &output_dir.display().to_string()])
        .expect("run mkdir output dir");
    fs::write(output_path, output_content.as_bytes())?;
    Ok(())
}

fn build_godwoken_scripts(repos_dir: &Path, repo_name: &str) {
    let repo_dir = make_target_dir(repos_dir, repo_name);
    let target_dir = format!("{}/c", repo_dir);
    run_command("make", vec!["-C", &target_dir]).expect("run make");
    run_command_in_dir(
        "capsule",
        vec!["build", "--release", "--debug-output"],
        &repo_dir,
    )
    .expect("run capsule build");
}

fn build_godwoken_polyjuice(repos_dir: &Path, repo_name: &str) {
    let target_dir = make_target_dir(repos_dir, repo_name);
    run_command("make", vec!["-C", &target_dir, "all-via-docker"]).expect("run make");
}

fn build_clerkb(repos_dir: &Path, repo_name: &str) {
    let target_dir = make_target_dir(repos_dir, repo_name);
    run_command("yarn", vec!["--cwd", &target_dir]).expect("run yarn");
    run_command("make", vec!["-C", &target_dir, "all-via-docker"]).expect("run make");
}

fn copy_scripts_to_target(repos_dir: &Path, scripts_dir: &Path) -> Result<()> {
    // RUN mkdir -p /scripts/godwoken-scripts
    // cp -a godwoken-scripts/build/release/. /scripts/godwoken-scripts/
    // cp -a godwoken-scripts/c/build/. /scripts/godwoken-scripts/
    // cp -a godwoken-scripts/c/build/account_locks/. /scripts/godwoken-scripts/
    let source_dir = make_target_dir(repos_dir, GODWOKEN_SCRIPTS);
    let target_dir = make_target_dir(scripts_dir, GODWOKEN_SCRIPTS);
    run_command("mkdir", vec!["-p", &target_dir])?;
    let source_file = format!("{}/build/release/.", source_dir);
    run_command("cp", vec!["-a", &source_file, &target_dir])?;
    let source_file = format!("{}/c/build/.", source_dir);
    run_command("cp", vec!["-a", &source_file, &target_dir])?;
    let source_file = format!("{}/c/build/account_locks/.", source_dir);
    run_command("cp", vec!["-a", &source_file, &target_dir])?;

    // mkdir -p /scripts/godwoken-polyjuice
    // cp godwoken-polyjuice/build/generator /scripts/godwoken-polyjuice/
    // cp godwoken-polyjuice/build/validator /scripts/godwoken-polyjuice/
    let source_dir = make_target_dir(repos_dir, GODWOKEN_POLYJUICE);
    let target_dir = make_target_dir(scripts_dir, GODWOKEN_POLYJUICE);
    run_command("mkdir", vec!["-p", &target_dir])?;
    let source_file = format!("{}/build/validator", source_dir);
    run_command("cp", vec![&source_file, &target_dir])?;
    let source_file = format!("{}/build/generator", source_dir);
    run_command("cp", vec![&source_file, &target_dir])?;

    // mkdir -p /scripts/clerkb
    // cp -a clerkb/build/debug/. /scripts/clerkb/
    let source_dir = make_target_dir(repos_dir, CLERKB);
    let target_dir = make_target_dir(scripts_dir, CLERKB);
    run_command("mkdir", vec!["-p", &target_dir])?;
    let source_file = format!("{}/build/debug/.", source_dir);
    run_command("cp", vec!["-a", &source_file, &target_dir])?;

    Ok(())
}

fn run_pull_code(
    mut repo_url: Url,
    is_recursive: bool,
    repos_dir: &Path,
    repo_name: &str,
) -> Result<()> {
    log::info!("Pull code of {} ...", repo_name);
    let commit = repo_url
        .fragment()
        .ok_or_else(|| anyhow::anyhow!("Invalid branch, commit, or tags."))?
        .to_owned();
    repo_url.set_fragment(None);
    let target_dir = make_target_dir(repos_dir, repo_name);
    if run_git_checkout(&target_dir, &commit).is_ok() {
        return Ok(());
    }
    run_command("rm", vec!["-rf", &target_dir]).expect("run rm dir");
    fs::create_dir_all(&target_dir).expect("create repo dir");
    run_git_clone(repo_url, is_recursive, &target_dir).expect("run git clone");
    run_git_checkout(&target_dir, &commit).expect("run git checkout");
    Ok(())
}

fn run_git_clone(repo_url: Url, is_recursive: bool, path: &str) -> Result<()> {
    let mut args = vec!["clone", repo_url.as_str(), path];
    if is_recursive {
        args.push("--recursive");
    }
    run_command("git", args)
}

fn run_git_checkout(repo_dir: &str, commit: &str) -> Result<()> {
    run_command("git", vec!["-C", repo_dir, "checkout", commit])?;
    run_command(
        "git",
        vec!["-C", &repo_dir, "submodule", "update", "--recursive"],
    )
}

fn run_command_in_dir<I, S>(bin: &str, args: I, target_dir: &str) -> Result<()>
where
    I: IntoIterator<Item = S> + std::fmt::Debug,
    S: AsRef<OsStr>,
{
    let working_dir = env::current_dir().expect("get working dir");
    env::set_current_dir(&target_dir).expect("set target dir");
    let result = run_command(bin, args);
    env::set_current_dir(&working_dir).expect("set working dir");
    result
}

fn run_command<I, S>(bin: &str, args: I) -> Result<()>
where
    I: IntoIterator<Item = S> + std::fmt::Debug,
    S: AsRef<OsStr>,
{
    log::info!("[Execute]: {} {:?}", bin, args);
    let status = Command::new(bin.to_owned())
        .env("RUST_BACKTRACE", "full")
        .args(args)
        .status()
        .expect("run command");
    if !status.success() {
        Err(anyhow::anyhow!(
            "Exited with status code: {:?}",
            status.code()
        ))
    } else {
        Ok(())
    }
}

fn make_target_dir(parent_dir_path: &Path, dir_name: &str) -> String {
    let mut output = PathBuf::from(parent_dir_path);
    output.push(dir_name);
    output.display().to_string()
}
