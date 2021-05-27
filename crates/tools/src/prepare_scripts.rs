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
const REPO_GODWOKEN_POLYJUICE: &str = "godwoken-polyjuice";
const REPO_CLERKB: &str = "clerkb";

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
    deposit_lock: PathBuf,
    withdrawal_lock: PathBuf,
    challenge_lock: PathBuf,
    stake_lock: PathBuf,
    // tron_account_lock: PathBuf,
    state_validator: PathBuf,
    sudt_generator: PathBuf,
    sudt_validator: PathBuf,
    meta_contract_validator: PathBuf,
    meta_contract_generator: PathBuf,
    eth_account_lock: PathBuf,

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
    log::info!("Build scripts...");
    run_pull_code(repos.godwoken_scripts, true, repos_dir, GODWOKEN_SCRIPTS)?;
    run_pull_code(
        repos.godwoken_polyjuice,
        true,
        repos_dir,
        REPO_GODWOKEN_POLYJUICE,
    )?;
    run_pull_code(repos.clerkb, true, repos_dir, REPO_CLERKB)?;
    build_godwoken_scripts(repos_dir, GODWOKEN_SCRIPTS);
    build_godwoken_polyjuice(repos_dir, REPO_GODWOKEN_POLYJUICE);
    build_clerkb(repos_dir, REPO_CLERKB);
    copy_scripts_to_target(repos_dir, scripts_dir)?;
    Ok(())
}

fn prepare_scripts_in_copy_mode(prebuild_image: &PathBuf, scripts_dir: &Path) {
    log::info!("Copy scritps from prebuild image...");
    let dummy = "dummy";
    run_command(
        "docker",
        vec![
            "create",
            "-ti",
            "--name",
            dummy,
            &prebuild_image.display().to_string(),
            "bash",
        ],
    )
    .expect("docker create container");
    let src_path_container = format!("{}:/scripts/.", dummy);
    run_command(
        "docker",
        vec![
            "cp",
            &src_path_container,
            &scripts_dir.display().to_string(),
        ],
    )
    .expect("docker cp files");
    run_command("docker", vec!["rm", "-f", dummy]).expect("docker rm container");
}

fn check_scripts_build_result(scripts_dir: &Path) -> BuildScripts {
    let build_scripts = BuildScripts {
        // scripts from godwoken-scripts
        always_success: make_path(scripts_dir, vec![GODWOKEN_SCRIPTS, "always-success"]),
        custodian_lock: make_path(scripts_dir, vec![GODWOKEN_SCRIPTS, "custodian-lock"]),
        deposit_lock: make_path(scripts_dir, vec![GODWOKEN_SCRIPTS, "deposition-lock"]),
        withdrawal_lock: make_path(scripts_dir, vec![GODWOKEN_SCRIPTS, "withdrawal-lock"]),
        challenge_lock: make_path(scripts_dir, vec![GODWOKEN_SCRIPTS, "challenge-lock"]),
        stake_lock: make_path(scripts_dir, vec![GODWOKEN_SCRIPTS, "stake-lock"]),
        state_validator: make_path(scripts_dir, vec![GODWOKEN_SCRIPTS, "state-validator"]),
        sudt_generator: make_path(scripts_dir, vec![GODWOKEN_SCRIPTS, "sudt-generator"]),
        sudt_validator: make_path(scripts_dir, vec![GODWOKEN_SCRIPTS, "sudt-validator"]),
        meta_contract_generator: make_path(
            scripts_dir,
            vec![GODWOKEN_SCRIPTS, "meta-contract-generator"],
        ),
        meta_contract_validator: make_path(
            scripts_dir,
            vec![GODWOKEN_SCRIPTS, "meta-contract-validator"],
        ),
        eth_account_lock: make_path(scripts_dir, vec![GODWOKEN_SCRIPTS, "eth-account-lock"]),

        // scripts from godwoken-Polyjuice
        polyjuice_generator: make_path(scripts_dir, vec![REPO_GODWOKEN_POLYJUICE, "generator"]),
        polyjuice_validator: make_path(scripts_dir, vec![REPO_GODWOKEN_POLYJUICE, "validator"]),

        // scripts from clerkb
        state_validator_lock: make_path(scripts_dir, vec![REPO_CLERKB, "poa"]),
        poa_state: make_path(scripts_dir, vec![REPO_CLERKB, "state"]),
    };

    // scripts from godwoken-scripts
    assert!(build_scripts.always_success.exists());
    assert!(build_scripts.custodian_lock.exists());
    assert!(build_scripts.deposit_lock.exists());
    assert!(build_scripts.withdrawal_lock.exists());
    assert!(build_scripts.challenge_lock.exists());
    assert!(build_scripts.stake_lock.exists());
    assert!(build_scripts.state_validator.exists());
    assert!(build_scripts.sudt_generator.exists());
    assert!(build_scripts.sudt_validator.exists());
    assert!(build_scripts.meta_contract_validator.exists());
    assert!(build_scripts.meta_contract_generator.exists());
    assert!(build_scripts.eth_account_lock.exists());

    // scripts from godwoken-Polyjuice
    assert!(build_scripts.polyjuice_generator.exists());
    assert!(build_scripts.polyjuice_validator.exists());

    // scripts from clerkb
    assert!(build_scripts.state_validator_lock.exists());
    assert!(build_scripts.poa_state.exists());

    build_scripts
}

fn generate_script_deploy_config(build_scripts: BuildScripts, output_path: &Path) -> Result<()> {
    log::info!("Generate scripts-deploy.json...");
    let programs = Programs {
        custodian_lock: build_scripts.custodian_lock.clone(),
        deposit_lock: build_scripts.deposit_lock.clone(),
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
    log::info!("Finish");
    Ok(())
}

fn build_godwoken_scripts(repos_dir: &Path, repo_name: &str) {
    let repo_dir = make_path(repos_dir, vec![repo_name]).display().to_string();
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
    let target_dir = make_path(repos_dir, vec![repo_name]).display().to_string();
    run_command("make", vec!["-C", &target_dir, "all-via-docker"]).expect("run make");
}

fn build_clerkb(repos_dir: &Path, repo_name: &str) {
    let target_dir = make_path(repos_dir, vec![repo_name]).display().to_string();
    run_command("yarn", vec!["--cwd", &target_dir]).expect("run yarn");
    run_command("make", vec!["-C", &target_dir, "all-via-docker"]).expect("run make");
}

fn copy_scripts_to_target(repos_dir: &Path, scripts_dir: &Path) -> Result<()> {
    // RUN mkdir -p /scripts/godwoken-scripts
    // cp -a godwoken-scripts/build/release/. /scripts/godwoken-scripts/
    // cp -a godwoken-scripts/c/build/. /scripts/godwoken-scripts/
    // cp -a godwoken-scripts/c/build/account_locks/. /scripts/godwoken-scripts/
    let source_dir = make_path(repos_dir, vec![GODWOKEN_SCRIPTS])
        .display()
        .to_string();
    let target_dir = make_path(scripts_dir, vec![GODWOKEN_SCRIPTS])
        .display()
        .to_string();
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
    let source_dir = make_path(repos_dir, vec![REPO_GODWOKEN_POLYJUICE])
        .display()
        .to_string();
    let target_dir = make_path(scripts_dir, vec![REPO_GODWOKEN_POLYJUICE])
        .display()
        .to_string();
    run_command("mkdir", vec!["-p", &target_dir])?;
    let source_file = format!("{}/build/validator", source_dir);
    run_command("cp", vec![&source_file, &target_dir])?;
    let source_file = format!("{}/build/generator", source_dir);
    run_command("cp", vec![&source_file, &target_dir])?;

    // mkdir -p /scripts/clerkb
    // cp -a clerkb/build/debug/. /scripts/clerkb/
    let source_dir = make_path(repos_dir, vec![REPO_CLERKB])
        .display()
        .to_string();
    let target_dir = make_path(scripts_dir, vec![REPO_CLERKB])
        .display()
        .to_string();
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
    let commit = repo_url
        .fragment()
        .ok_or_else(|| anyhow::anyhow!("Invalid branch, commit, or tags."))?
        .to_owned();
    repo_url.set_fragment(None);
    let target_dir = make_path(repos_dir, vec![repo_name]).display().to_string();
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

fn make_path<P: AsRef<Path>>(parent_dir_path: &Path, paths: Vec<P>) -> PathBuf {
    let mut target = PathBuf::from(parent_dir_path);
    for p in paths {
        target.push(p);
    }
    target
}
