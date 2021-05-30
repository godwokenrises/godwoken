use crate::deploy_scripts::Programs;
use anyhow::Result;
use ckb_fixed_hash::H256;
use ckb_jsonrpc_types::{JsonBytes, Script, ScriptHashType};
use clap::arg_enum;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    env,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use url::Url;

const GODWOKEN_SCRIPTS: &'static str = "godwoken-scripts";
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
    prebuild_image: PathBuf,
    repos: ReposUrl,
    scripts: HashMap<String, ScriptsInfo>,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug)]
struct ScriptsInfo {
    source: PathBuf,
    deploy: DeployOption,
}

impl ScriptsInfo {
    fn source_script_path(&self, repos_dir: &Path) -> PathBuf {
        make_path(repos_dir, vec![self.source.as_path()])
    }

    fn target_script_path(&self, target_root_dir: &Path) -> PathBuf {
        let script_name = self.source.file_name().expect("get script name");
        let repo_name = self
            .source
            .components()
            .next()
            .expect("get repo name")
            .as_os_str();
        make_path(target_root_dir, vec![repo_name, script_name])
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug)]
enum DeployOption {
    Yes,
    No,
    Success,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug)]
struct ReposUrl {
    godwoken_scripts: Url,
    godwoken_polyjuice: Url,
    clerkb: Url,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug)]
struct BuildScriptsResult {
    programs: Programs,
    lock: Script,
    built_scripts: HashMap<String, PathBuf>,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug)]
struct ScriptPath {
    repos_dir: PathBuf,
    repo_name: String,
    source_build_dir: PathBuf,
    target_root_dir: PathBuf,
}

pub fn prepare_scripts(
    mode: ScriptsBuildMode,
    input_path: &Path,
    repos_dir: &Path,
    scripts_dir: &Path,
    output_path: &Path,
) -> Result<()> {
    let input = fs::read_to_string(input_path)?;
    let scripts_build_config: ScriptsBuildConfig =
        serde_json::from_str(input.as_str()).expect("parse scripts build config");
    match mode {
        ScriptsBuildMode::Build => {
            prepare_scripts_in_build_mode(&scripts_build_config, repos_dir, scripts_dir);
        }
        ScriptsBuildMode::Copy => {
            prepare_scripts_in_copy_mode(&scripts_build_config.prebuild_image, scripts_dir);
        }
    }
    check_scripts(&scripts_dir, &scripts_build_config.scripts);
    generate_script_deploy_config(scripts_dir, &scripts_build_config.scripts, output_path)
}

fn prepare_scripts_in_build_mode(
    scripts_build_config: &ScriptsBuildConfig,
    repos_dir: &Path,
    target_dir: &Path,
) {
    log::info!("Build scripts...");
    run_pull_code(
        scripts_build_config.repos.godwoken_scripts.clone(),
        true,
        repos_dir,
        GODWOKEN_SCRIPTS,
    );
    run_pull_code(
        scripts_build_config.repos.godwoken_polyjuice.clone(),
        true,
        repos_dir,
        GODWOKEN_POLYJUICE,
    );
    run_pull_code(
        scripts_build_config.repos.clerkb.clone(),
        true,
        repos_dir,
        CLERKB,
    );
    build_godwoken_scripts(repos_dir, GODWOKEN_SCRIPTS);
    build_godwoken_polyjuice(repos_dir, GODWOKEN_POLYJUICE);
    build_clerkb(repos_dir, CLERKB);
    copy_scripts_to_target(repos_dir, target_dir, &scripts_build_config.scripts);
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

fn check_scripts(target_dir: &Path, scripts_info: &HashMap<String, ScriptsInfo>) {
    scripts_info.iter().for_each(|(_, v)| {
        let target_path = v.target_script_path(target_dir);
        assert!(
            target_path.exists(),
            "script does not exist: {:?}",
            target_path
        );
    });
}

fn generate_script_deploy_config(
    target_dir: &Path,
    scripts_info: &HashMap<String, ScriptsInfo>,
    output_path: &Path,
) -> Result<()> {
    log::info!("Generate scripts-deploy.json...");
    let _always_success = scripts_info
        .get("always_success")
        .expect("get script info")
        .target_script_path(target_dir);
    let programs = Programs {
        custodian_lock: scripts_info
            .get("custodian_lock")
            .expect("get script info")
            .target_script_path(target_dir),
        deposit_lock: scripts_info
            .get("deposit_lock")
            .expect("get script info")
            .target_script_path(target_dir),
        withdrawal_lock: scripts_info
            .get("withdrawal_lock")
            .expect("get script info")
            .target_script_path(target_dir),
        challenge_lock: scripts_info
            .get("challenge_lock")
            .expect("get script info")
            .target_script_path(target_dir),
        stake_lock: scripts_info
            .get("stake_lock")
            .expect("get script info")
            .target_script_path(target_dir),
        state_validator: scripts_info
            .get("state_validator")
            .expect("get script info")
            .target_script_path(target_dir),
        l2_sudt_validator: scripts_info
            .get("l2_sudt_validator")
            .expect("get script info")
            .target_script_path(target_dir),
        eth_account_lock: scripts_info
            .get("eth_account_lock")
            .expect("get script info")
            .target_script_path(target_dir),
        tron_account_lock: scripts_info
            .get("tron_account_lock")
            .expect("get script info")
            .target_script_path(target_dir),
        meta_contract_validator: scripts_info
            .get("meta_contract_validator")
            .expect("get script info")
            .target_script_path(target_dir),
        polyjuice_validator: scripts_info
            .get("polyjuice_validator")
            .expect("get script info")
            .target_script_path(target_dir),
        state_validator_lock: scripts_info
            .get("state_validator_lock")
            .expect("get script info")
            .target_script_path(target_dir),
        poa_state: scripts_info
            .get("poa_state")
            .expect("get script info")
            .target_script_path(target_dir),
    };
    let build_scripts_result = BuildScriptsResult {
        programs,
        lock: Script {
            code_hash: H256::default(),
            hash_type: ScriptHashType::Data,
            args: JsonBytes::default(),
        },
        built_scripts: scripts_info
            .iter()
            .map(|(k, v)| (k.clone(), v.target_script_path(target_dir)))
            .collect(),
    };
    let output_content =
        serde_json::to_string_pretty(&build_scripts_result).expect("serde json to string pretty");
    let output_dir = output_path.parent().expect("get output dir");
    fs::create_dir_all(&output_dir).expect("create output dir");
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

fn copy_scripts_to_target(
    repos_dir: &Path,
    target_dir: &Path,
    scripts_info: &HashMap<String, ScriptsInfo>,
) {
    scripts_info.iter().for_each(|(_, v)| {
        let target_path = v.target_script_path(target_dir);
        fs::create_dir_all(&target_path.parent().expect("get dir")).expect("create scripts dir");
        fs::copy(v.source_script_path(repos_dir), &target_path).expect("copy script");
    });
}

fn run_pull_code(mut repo_url: Url, is_recursive: bool, repos_dir: &Path, repo_name: &str) {
    let commit = repo_url
        .fragment()
        .expect("valid branch, tag, or commit")
        .to_owned();
    repo_url.set_fragment(None);
    let target_dir = make_path(repos_dir, vec![repo_name]);
    if run_git_checkout(&target_dir.display().to_string(), &commit).is_ok() {
        return;
    }
    if target_dir.exists() {
        fs::remove_dir_all(&target_dir).expect("clean repo dir");
    }
    fs::create_dir_all(&target_dir).expect("create repo dir");
    run_git_clone(repo_url, is_recursive, &target_dir.display().to_string())
        .expect("run git clone");
    run_git_checkout(&target_dir.display().to_string(), &commit).expect("run git checkout");
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
