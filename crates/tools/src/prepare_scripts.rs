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

const GODWOKEN_SCRIPTS: &str = "godwoken-scripts";
const GODWOKEN_POLYJUICE: &str = "godwoken-polyjuice";
const CLERKB: &str = "clerkb";

static DEFAULT_BUILD_CONFIG: &str = r#" {
    "prebuild_image": "nervos/godwoken-prebuilds:v0.3.0",
    "repos": {
        "godwoken_scripts": "https://github.com/nervosnetwork/godwoken-scripts#v0.5.0-rc1",
        "godwoken_polyjuice": "https://github.com/nervosnetwork/godwoken-polyjuice#v0.6.0-rc6",
        "clerkb": "https://github.com/nervosnetwork/clerkb#v0.4.0"
    },
    "scripts": {
        "always_success": { "source": "godwoken-scripts/build/release/always-success" },
        "custodian_lock": { "source": "godwoken-scripts/build/release/custodian-lock" },
        "deposit_lock": { "source": "godwoken-scripts/build/release/deposit-lock" },
        "withdrawal_lock":  {"source": "godwoken-scripts/build/release/withdrawal-lock" },
        "challenge_lock": { "source": "godwoken-scripts/build/release/challenge-lock" },
        "stake_lock": { "source": "godwoken-scripts/build/release/stake-lock" },
        "tron_account_lock": { "source": "godwoken-scripts/build/release/always-success" },
        "state_validator": { "source": "godwoken-scripts/build/release/state-validator" },
        "eth_account_lock": { "source": "godwoken-scripts/build/release/eth-account-lock" },

        "l2_sudt_generator": { "source": "godwoken-scripts/c/build/sudt-generator" },
        "l2_sudt_validator": { "source": "godwoken-scripts/c/build/sudt-validator" },
        "meta_contract_generator": { "source": "godwoken-scripts/c/build/meta-contract-generator" },
        "meta_contract_validator": { "source": "godwoken-scripts/c/build/meta-contract-validator" },
        
        "polyjuice_generator": { "source": "godwoken-polyjuice/build/generator" },
        "polyjuice_validator": { "source": "godwoken-polyjuice/build/validator" },
        "state_validator_lock": { "source": "clerkb/build/debug/poa" },
        "poa_state": { "source": "clerkb/build/debug/state" }
    }
} "#;

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

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug)]
struct BuildScriptsResult {
    programs: Programs,
    lock: Script,
    built_scripts: HashMap<String, PathBuf>,
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

pub fn prepare_scripts(
    mode: ScriptsBuildMode,
    input_path: &Path,
    repos_dir: &Path,
    scripts_dir: &Path,
    output_path: &Path,
) -> Result<()> {
    let scripts_build_config = read_script_build_config(input_path);
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

fn read_script_build_config<P: AsRef<Path>>(input_path: P) -> ScriptsBuildConfig {
    let input = fs::read_to_string(input_path).expect("read config file");
    let mut scripts_build_config: ScriptsBuildConfig =
        serde_json::from_str(&input).expect("parse scripts build config");
    let default_build_config: ScriptsBuildConfig =
        serde_json::from_str(&DEFAULT_BUILD_CONFIG).expect("parse scripts build config");
    default_build_config.scripts.iter().for_each(|(k, v)| {
        match scripts_build_config.scripts.get(k) {
            Some(value) => {
                let mut new = value.to_owned();
                if PathBuf::default() == new.source {
                    new.source.clone_from(&v.source);
                }
                new.always_success = value.always_success;
                scripts_build_config.scripts.insert(k.to_owned(), new);
            }
            None => {
                scripts_build_config
                    .scripts
                    .insert(k.to_owned(), v.to_owned());
            }
        }
    });
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
    run_command("git", vec!["-C", repo_dir, "fetch"])?;
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
