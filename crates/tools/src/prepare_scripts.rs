use crate::{
    types::{BuildScriptsResult, Programs},
    utils,
};
use anyhow::Result;
use clap::arg_enum;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};
use url::Url;

pub const SCRIPT_BUILD_DIR_PATH: &str = "scripts-build/";
pub const SCRIPTS_DIR_PATH: &str = "scripts/";
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
    prebuild_image: PathBuf,
    repos: ReposUrl,

    #[serde(default)]
    scripts: HashMap<String, ScriptsInfo>,
}

impl Default for ScriptsBuildConfig {
    fn default() -> Self {
        ScriptsBuildConfig {
            prebuild_image: PathBuf::from("nervos/godwoken-prebuilds:v0.6.7-rc1"),
            repos: ReposUrl {
                godwoken_scripts: Url::parse(
                    "https://github.com/nervosnetwork/godwoken-scripts#v0.8.4-rc1",
                )
                .expect("url parse"),
                godwoken_polyjuice: Url::parse(
                    "https://github.com/nervosnetwork/godwoken-polyjuice#v0.8.8",
                )
                .expect("url parse"),
                clerkb: Url::parse("https://github.com/nervosnetwork/clerkb#v0.4.0")
                    .expect("url parse"),
            },
            scripts: [
                (
                    "always_success",
                    "godwoken-scripts/build/release/always-success",
                ),
                (
                    "custodian_lock",
                    "godwoken-scripts/build/release/custodian-lock",
                ),
                (
                    "deposit_lock",
                    "godwoken-scripts/build/release/deposit-lock",
                ),
                (
                    "withdrawal_lock",
                    "godwoken-scripts/build/release/withdrawal-lock",
                ),
                (
                    "challenge_lock",
                    "godwoken-scripts/build/release/challenge-lock",
                ),
                ("stake_lock", "godwoken-scripts/build/release/stake-lock"),
                (
                    "tron_account_lock",
                    "godwoken-scripts/build/release/tron-account-lock",
                ),
                (
                    "state_validator",
                    "godwoken-scripts/build/release/state-validator",
                ),
                (
                    "eth_account_lock",
                    "godwoken-scripts/build/release/eth-account-lock",
                ),
                (
                    "l2_sudt_generator",
                    "godwoken-scripts/c/build/sudt-generator",
                ),
                (
                    "l2_sudt_validator",
                    "godwoken-scripts/c/build/sudt-validator",
                ),
                (
                    "meta_contract_generator",
                    "godwoken-scripts/c/build/meta-contract-generator",
                ),
                (
                    "meta_contract_validator",
                    "godwoken-scripts/c/build/meta-contract-validator",
                ),
                ("polyjuice_generator", "godwoken-polyjuice/build/generator"),
                ("polyjuice_validator", "godwoken-polyjuice/build/validator"),
                ("state_validator_lock", "clerkb/build/debug/poa"),
                ("poa_state", "clerkb/build/debug/state"),
            ]
            .iter()
            .map(|(k, v)| {
                (
                    k.to_string(),
                    ScriptsInfo {
                        source: PathBuf::from(v),
                        always_success: false,
                    },
                )
            })
            .collect(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug)]
struct ReposUrl {
    godwoken_scripts: Url,
    godwoken_polyjuice: Url,
    clerkb: Url,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug)]
struct ScriptsInfo {
    #[serde(default)]
    source: PathBuf,

    #[serde(default)]
    always_success: bool,
}

impl ScriptsInfo {
    fn source_script_path(&self, repos_dir: &Path) -> PathBuf {
        let mut p = PathBuf::default();
        p.push(repos_dir);
        p.push(self.source.as_path());
        p
    }

    fn target_script_path(&self, target_root_dir: &Path) -> PathBuf {
        let script_name = self.source.file_name().expect("get script name");
        let repo_name = self
            .source
            .components()
            .next()
            .expect("get repo name")
            .as_os_str();
        let mut p = PathBuf::from(target_root_dir);
        p.push(repo_name);
        p.push(script_name);
        p
    }
}

pub fn prepare_scripts(
    mode: ScriptsBuildMode,
    scripts_lock: ckb_jsonrpc_types::Script,
    build_config_path: &Path,
    build_dir: &Path,
    scripts_output_dir: &Path,
) -> Result<BuildScriptsResult> {
    let scripts_build_config = read_script_build_config(build_config_path);
    match mode {
        ScriptsBuildMode::Build => {
            prepare_scripts_in_build_mode(&scripts_build_config, build_dir, scripts_output_dir);
        }
        ScriptsBuildMode::Copy => {
            prepare_scripts_in_copy_mode(scripts_build_config.prebuild_image, scripts_output_dir);
        }
    }
    check_scripts(scripts_output_dir, &scripts_build_config.scripts);
    generate_script_deploy_config(
        scripts_output_dir,
        scripts_lock,
        &scripts_build_config.scripts,
    )
}

fn read_script_build_config<P: AsRef<Path>>(input_path: P) -> ScriptsBuildConfig {
    let input = fs::read_to_string(input_path).expect("read config file");
    let mut scripts_build_config: ScriptsBuildConfig =
        serde_json::from_str(&input).expect("parse scripts build config");
    let default_build_config: ScriptsBuildConfig = ScriptsBuildConfig::default();
    default_build_config
        .scripts
        .iter()
        .for_each(
            |(key, default_value)| match scripts_build_config.scripts.get(key) {
                Some(value) => {
                    if PathBuf::default() == value.source {
                        let mut new = value.to_owned();
                        new.source.clone_from(&default_value.source);
                        scripts_build_config.scripts.insert(key.to_owned(), new);
                    }
                }
                None => {
                    scripts_build_config
                        .scripts
                        .insert(key.to_owned(), default_value.to_owned());
                }
            },
        );
    scripts_build_config
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
    collect_scripts_to_target(repos_dir, target_dir, &scripts_build_config.scripts);
}

fn prepare_scripts_in_copy_mode(prebuild_image: PathBuf, scripts_dir: &Path) {
    log::info!("Copy scritps from prebuild image...");
    let dummy = "dummy";
    utils::transaction::run(
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
    utils::transaction::run(
        "docker",
        vec![
            "cp",
            &src_path_container,
            &scripts_dir.display().to_string(),
        ],
    )
    .expect("docker cp files");
    utils::transaction::run("docker", vec!["rm", "-f", dummy]).expect("docker rm container");
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
    scripts_lock: ckb_jsonrpc_types::Script,
    scripts_info: &HashMap<String, ScriptsInfo>,
) -> Result<BuildScriptsResult> {
    let always_success = scripts_info
        .get("always_success")
        .expect("get script info")
        .target_script_path(target_dir);
    let get_path = |script: &str| {
        let script_info = scripts_info.get(script).expect("get script info");
        if script_info.always_success {
            always_success.to_owned()
        } else {
            script_info.target_script_path(target_dir)
        }
    };
    let programs = Programs {
        custodian_lock: get_path("custodian_lock"),
        deposit_lock: get_path("deposit_lock"),
        withdrawal_lock: get_path("withdrawal_lock"),
        challenge_lock: get_path("challenge_lock"),
        stake_lock: get_path("stake_lock"),
        state_validator: get_path("state_validator"),
        l2_sudt_validator: get_path("l2_sudt_validator"),
        eth_account_lock: get_path("eth_account_lock"),
        tron_account_lock: get_path("tron_account_lock"),
        meta_contract_validator: get_path("meta_contract_validator"),
        polyjuice_validator: get_path("polyjuice_validator"),
        state_validator_lock: get_path("state_validator_lock"),
        poa_state: get_path("poa_state"),
    };
    let build_scripts_result = BuildScriptsResult {
        programs,
        lock: scripts_lock,
        built_scripts: scripts_info
            .iter()
            .map(|(k, v)| (k.clone(), v.target_script_path(target_dir)))
            .collect(),
    };
    Ok(build_scripts_result)
}

fn build_godwoken_scripts(repos_dir: &Path, repo_name: &str) {
    let repo_dir = repos_dir.join(repo_name).display().to_string();
    let target_dir = format!("{}/c", repo_dir);
    utils::transaction::run("make", vec!["-C", &target_dir]).expect("run make");
    utils::transaction::run_in_dir(
        "capsule",
        vec!["build", "--release", "--debug-output"],
        &repo_dir,
    )
    .expect("run capsule build");
}

fn build_godwoken_polyjuice(repos_dir: &Path, repo_name: &str) {
    let target_dir = repos_dir.join(repo_name).display().to_string();
    utils::transaction::run("make", vec!["-C", &target_dir, "all-via-docker"]).expect("run make");
}

fn build_clerkb(repos_dir: &Path, repo_name: &str) {
    let target_dir = repos_dir.join(repo_name).display().to_string();
    utils::transaction::run("yarn", vec!["--cwd", &target_dir]).expect("run yarn");
    utils::transaction::run("make", vec!["-C", &target_dir, "all-via-docker"]).expect("run make");
}

fn collect_scripts_to_target(
    repos_dir: &Path,
    target_dir: &Path,
    scripts_info: &HashMap<String, ScriptsInfo>,
) {
    scripts_info.iter().for_each(|(_, v)| {
        let target_path = v.target_script_path(target_dir);
        let source_path = v.source_script_path(repos_dir);
        fs::create_dir_all(&target_path.parent().expect("get dir")).expect("create scripts dir");
        log::debug!("copy {:?} to {:?}", source_path, target_path);
        fs::copy(source_path, target_path).expect("copy script");
    });
}

fn run_pull_code(mut repo_url: Url, is_recursive: bool, repos_dir: &Path, repo_name: &str) {
    let commit = repo_url
        .fragment()
        .expect("valid branch, tag, or commit")
        .to_owned();
    repo_url.set_fragment(None);
    let target_dir = repos_dir.join(repo_name);
    if target_dir.exists() {
        if run_git_checkout(&target_dir.display().to_string(), &commit).is_ok() {
            return;
        }
        log::info!("Run git checkout failed, the repo will re-init...");
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
    utils::transaction::run("git", args)
}

fn run_git_checkout(repo_dir: &str, commit: &str) -> Result<()> {
    utils::transaction::run("git", vec!["-C", repo_dir, "fetch"])?;
    utils::transaction::run("git", vec!["-C", repo_dir, "checkout", commit])?;
    utils::transaction::run(
        "git",
        vec!["-C", repo_dir, "submodule", "update", "--recursive"],
    )
}
