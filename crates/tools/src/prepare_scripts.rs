use anyhow::Result;
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

pub fn prepare_scripts(
    mode: ScriptsBuildMode,
    input_path: &Path,
    repos_dir: &Path,
    scripts_dir: &Path,
    _output_path: &Path,
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
    generate_script_deploy_config(_output_path);
    Ok(())
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
    build_godwoken_scripts(repos_dir, GODWOKEN_SCRIPTS)?;
    build_godwoken_polyjuice(repos_dir, GODWOKEN_POLYJUICE)?;
    build_clerkb(repos_dir, CLERKB)?;
    copy_scripts_to_target(repos_dir, scripts_dir)?;
    Ok(())
}

fn prepare_scripts_in_copy_mode(prebuild_image: &PathBuf, scripts_dir: &Path) {
    let current_dir = env::current_dir().expect("Get working dir failed");
    let target_dir = make_target_dir(&current_dir, &scripts_dir.display().to_string());
    let volumn_arg = format!("-v{}:/{}", target_dir, "temp");
    run_command(
        "docker",
        vec![
            "run",
            "--rm",
            &volumn_arg,
            &prebuild_image.display().to_string(),
            "cp",
            "-r",
            "scripts/.",
            "temp",
        ],
    )
    .expect("Run docker cp failed");
}

fn generate_script_deploy_config(_output_path: &Path) {}

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
    run_command("rm", vec!["-rf", &target_dir]).expect("Run rm dir failed");
    fs::create_dir_all(&target_dir).expect("Create dir failed");
    run_git_clone(repo_url, is_recursive, &target_dir).expect("Run git clone failed");
    run_git_checkout(&target_dir, &commit).expect("Run git checkout failed");
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

fn build_godwoken_scripts(repos_dir: &Path, repo_name: &str) -> Result<()> {
    let repo_dir = make_target_dir(repos_dir, repo_name);
    let target_dir = format!("{}/c", repo_dir);
    run_command("make", vec!["-C", &target_dir]).expect("Run make failed");
    run_command_in_dir(
        "capsule",
        vec!["build", "--release", "--debug-output"],
        &repo_dir,
    )
    .expect("Run capsule build failed");
    Ok(())
}

fn build_godwoken_polyjuice(repos_dir: &Path, repo_name: &str) -> Result<()> {
    let target_dir = make_target_dir(repos_dir, repo_name);
    run_command("make", vec!["-C", &target_dir, "all-via-docker"]).expect("Run make failed");
    Ok(())
}

fn build_clerkb(repos_dir: &Path, repo_name: &str) -> Result<()> {
    let target_dir = make_target_dir(repos_dir, repo_name);
    run_command("yarn", vec!["--cwd", &target_dir]).expect("Run yarn failed");
    run_command("make", vec!["-C", &target_dir, "all-via-docker"]).expect("Run make failed");
    Ok(())
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

fn run_command_in_dir<I, S>(bin: &str, args: I, target_dir: &str) -> Result<()>
where
    I: IntoIterator<Item = S> + std::fmt::Debug,
    S: AsRef<OsStr>,
{
    let working_dir = env::current_dir().expect("Get working dir failed");
    env::set_current_dir(&target_dir).expect("Set target dir failed");
    let result = run_command(bin, args);
    env::set_current_dir(&working_dir).expect("Set working dir failed");
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
        .expect("Run command failed");
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
