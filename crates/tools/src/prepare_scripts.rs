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

const SCRIPT_ALWAYS_SUCCESS: &str = "always-success";
const SCRIPT_CUSTODIAN_LOCK: &str = "custodian-lock";
const SCRIPT_DEPOSIT_LOCK: &str = "deposition-lock"; // need rename
const SCRIPT_WITHDRAWAL: &str = "withdrawal-lock";
const SCRIPT_CHALLENGE_LOCK: &str = "challenge-lock";
const SCRIPT_STAKE_LOCK: &str = "stake-lock";
// const SCRIPT_TRON_ACCOUNT_LOCK = "tron-account-lock",
const SCRIPT_STATE_VALIDATOR: &str = "state-validator";
const SCRIPT_SUDT_GENERATOR: &str = "sudt-generator";
const SCRIPT_SUDT_VALIDATOR: &str = "sudt-validator";
const SCRIPT_META_CONTRACT_GENERATOR: &str = "meta-contract-generator";
const SCRIPT_META_CONTRACT_VALIDATOR: &str = "meta-contract-validator";
const SCRIPT_ETH_ACCOUNT_LOCK: &str = "eth-account-lock";
const SCRIPT_POLYJUICE_GENERATOR: &str = "generator";
const SCRIPT_POLYJUICE_VALIDATOR: &str = "validator";
const SCRIPT_POA: &str = "poa";
const SCRIPT_POA_STATE: &str = "state";

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
    build_scripts: HashMap<String, PathBuf>,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug)]
struct ScriptPath {
    repos_dir: PathBuf,
    repo_name: String,
    source_build_dir: PathBuf,
    target_root_dir: PathBuf,
}

impl ScriptPath {
    fn new(
        repos_dir: PathBuf,
        repo_name: String,
        source_build_dir: PathBuf,
        target_root_dir: PathBuf,
    ) -> Self {
        ScriptPath {
            repos_dir,
            repo_name,
            source_build_dir,
            target_root_dir,
        }
    }

    fn source_script_path(&self, script_name: &str) -> PathBuf {
        make_path(
            &self.repos_dir,
            vec![
                self.repo_name.as_str(),
                &self.source_build_dir.display().to_string(),
                script_name,
            ],
        )
    }

    fn target_script_path(&self, script_name: &str) -> PathBuf {
        make_path(
            &self.target_root_dir,
            vec![self.repo_name.as_str(), script_name],
        )
    }
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
    let scripts_paths = generate_scripts_paths(repos_dir, scripts_dir);
    match mode {
        ScriptsBuildMode::Build => {
            prepare_scripts_in_build_mode(scripts_build_config.repos, repos_dir, &scripts_paths);
        }
        ScriptsBuildMode::Copy => {
            prepare_scripts_in_copy_mode(&scripts_build_config.prebuild_image, scripts_dir);
        }
    }
    check_scripts(&scripts_paths);
    generate_script_deploy_config(scripts_paths, output_path)
}

fn generate_scripts_paths(repos_dir: &Path, scripts_dir: &Path) -> HashMap<String, ScriptPath> {
    let mut map = HashMap::new();
    map.insert(
        SCRIPT_ALWAYS_SUCCESS.to_owned(),
        ScriptPath::new(
            repos_dir.into(),
            GODWOKEN_SCRIPTS.to_owned(),
            "build/release/".into(),
            scripts_dir.into(),
        ),
    );
    map.insert(
        SCRIPT_CUSTODIAN_LOCK.to_owned(),
        ScriptPath::new(
            repos_dir.into(),
            GODWOKEN_SCRIPTS.to_owned(),
            "build/release/".into(),
            scripts_dir.into(),
        ),
    );
    map.insert(
        SCRIPT_DEPOSIT_LOCK.to_owned(),
        ScriptPath::new(
            repos_dir.into(),
            GODWOKEN_SCRIPTS.to_owned(),
            "build/release/".into(),
            scripts_dir.into(),
        ),
    );
    map.insert(
        SCRIPT_WITHDRAWAL.to_owned(),
        ScriptPath::new(
            repos_dir.into(),
            GODWOKEN_SCRIPTS.to_owned(),
            "build/release/".into(),
            scripts_dir.into(),
        ),
    );
    map.insert(
        SCRIPT_CHALLENGE_LOCK.to_owned(),
        ScriptPath::new(
            repos_dir.into(),
            GODWOKEN_SCRIPTS.to_owned(),
            "build/release/".into(),
            scripts_dir.into(),
        ),
    );
    map.insert(
        SCRIPT_STAKE_LOCK.to_owned(),
        ScriptPath::new(
            repos_dir.into(),
            GODWOKEN_SCRIPTS.to_owned(),
            "build/release/".into(),
            scripts_dir.into(),
        ),
    );
    map.insert(
        SCRIPT_STATE_VALIDATOR.to_owned(),
        ScriptPath::new(
            repos_dir.into(),
            GODWOKEN_SCRIPTS.to_owned(),
            "build/release/".into(),
            scripts_dir.into(),
        ),
    );
    map.insert(
        SCRIPT_META_CONTRACT_GENERATOR.to_owned(),
        ScriptPath::new(
            repos_dir.into(),
            GODWOKEN_SCRIPTS.to_owned(),
            "c/build/".into(),
            scripts_dir.into(),
        ),
    );
    map.insert(
        SCRIPT_META_CONTRACT_VALIDATOR.to_owned(),
        ScriptPath::new(
            repos_dir.into(),
            GODWOKEN_SCRIPTS.to_owned(),
            "c/build/".into(),
            scripts_dir.into(),
        ),
    );
    map.insert(
        SCRIPT_SUDT_GENERATOR.to_owned(),
        ScriptPath::new(
            repos_dir.into(),
            GODWOKEN_SCRIPTS.to_owned(),
            "c/build/".into(),
            scripts_dir.into(),
        ),
    );
    map.insert(
        SCRIPT_SUDT_VALIDATOR.to_owned(),
        ScriptPath::new(
            repos_dir.into(),
            GODWOKEN_SCRIPTS.to_owned(),
            "c/build/".into(),
            scripts_dir.into(),
        ),
    );
    map.insert(
        SCRIPT_ETH_ACCOUNT_LOCK.to_owned(),
        ScriptPath::new(
            repos_dir.into(),
            GODWOKEN_SCRIPTS.to_owned(),
            "c/build/account_locks/".into(),
            scripts_dir.into(),
        ),
    );
    map.insert(
        SCRIPT_POLYJUICE_GENERATOR.to_owned(),
        ScriptPath::new(
            repos_dir.into(),
            GODWOKEN_POLYJUICE.to_owned(),
            "build/".into(),
            scripts_dir.into(),
        ),
    );
    map.insert(
        SCRIPT_POLYJUICE_VALIDATOR.to_owned(),
        ScriptPath::new(
            repos_dir.into(),
            GODWOKEN_POLYJUICE.to_owned(),
            "build/".into(),
            scripts_dir.into(),
        ),
    );
    map.insert(
        SCRIPT_POA.to_owned(),
        ScriptPath::new(
            repos_dir.into(),
            CLERKB.to_owned(),
            "build/debug/".into(),
            scripts_dir.into(),
        ),
    );
    map.insert(
        SCRIPT_POA_STATE.to_owned(),
        ScriptPath::new(
            repos_dir.into(),
            CLERKB.to_owned(),
            "build/debug/".into(),
            scripts_dir.into(),
        ),
    );
    map
}

fn prepare_scripts_in_build_mode(
    repos: Repos,
    repos_dir: &Path,
    scripts_paths: &HashMap<String, ScriptPath>,
) {
    log::info!("Build scripts...");
    run_pull_code(repos.godwoken_scripts, true, repos_dir, GODWOKEN_SCRIPTS);
    run_pull_code(
        repos.godwoken_polyjuice,
        true,
        repos_dir,
        GODWOKEN_POLYJUICE,
    );
    run_pull_code(repos.clerkb, true, repos_dir, CLERKB);
    build_godwoken_scripts(repos_dir, GODWOKEN_SCRIPTS);
    build_godwoken_polyjuice(repos_dir, GODWOKEN_POLYJUICE);
    build_clerkb(repos_dir, CLERKB);
    copy_scripts_to_target(scripts_paths);
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

fn check_scripts(scripts_paths: &HashMap<String, ScriptPath>) {
    scripts_paths.iter().for_each(|(k, v)| {
        assert!(v.target_script_path(k).exists(), "{:?}", v.target_script_path(k));
    });
}

fn generate_script_deploy_config(
    build_scripts: HashMap<String, ScriptPath>,
    output_path: &Path,
) -> Result<()> {
    log::info!("Generate scripts-deploy.json...");
    let script_always_success = build_scripts
        .get(SCRIPT_ALWAYS_SUCCESS)
        .expect("get script path")
        .target_script_path(SCRIPT_ALWAYS_SUCCESS);
    let programs = Programs {
        custodian_lock: build_scripts
            .get(SCRIPT_CUSTODIAN_LOCK)
            .expect("get script path")
            .target_script_path(SCRIPT_CUSTODIAN_LOCK),
        deposit_lock: build_scripts
            .get(SCRIPT_DEPOSIT_LOCK)
            .expect("get script path")
            .target_script_path(SCRIPT_DEPOSIT_LOCK),
        withdrawal_lock: build_scripts
            .get(SCRIPT_WITHDRAWAL)
            .expect("get script path")
            .target_script_path(SCRIPT_WITHDRAWAL),
        challenge_lock: script_always_success.clone(), // always_success
        stake_lock: build_scripts
            .get(SCRIPT_STAKE_LOCK)
            .expect("get script path")
            .target_script_path(SCRIPT_STAKE_LOCK),
        state_validator: script_always_success.clone(), // always_success
        l2_sudt_validator: script_always_success.clone(), // always_success
        eth_account_lock: script_always_success.clone(), // always_success
        tron_account_lock: script_always_success.clone(), // always_success
        meta_contract_validator: script_always_success.clone(), // always_success
        polyjuice_validator: script_always_success.clone(), // always_success
        state_validator_lock: build_scripts
            .get(SCRIPT_STATE_VALIDATOR)
            .expect("get script path")
            .target_script_path(SCRIPT_STATE_VALIDATOR),
        poa_state: build_scripts
            .get(SCRIPT_POA)
            .expect("get script path")
            .target_script_path(SCRIPT_POA),
    };
    let lock = Script {
        code_hash: H256::default(),
        hash_type: ScriptHashType::Data,
        args: JsonBytes::default(),
    };
    let build_scripts_result = BuildScriptsResult {
        programs,
        lock,
        build_scripts: build_scripts
            .into_iter()
            .map(|(k, v)| (k.clone(), v.target_script_path(&k)))
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

fn copy_scripts_to_target(scripts_paths: &HashMap<String, ScriptPath>) {
    scripts_paths.iter().for_each(|(k, v)| {
        let target_path = v.target_script_path(k);
        fs::create_dir_all(&target_path.parent().expect("get dir")).expect("create scripts dir");
        fs::copy(v.source_script_path(k), &target_path).expect("copy script");
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
