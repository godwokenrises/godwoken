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

fn prepare_scripts_in_build_mode(
    repos: Repos,
    repos_dir: &Path,
    _scripts_dir: &Path,
) -> Result<()> {
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
    Ok(())
}

fn prepare_scripts_in_copy_mode(prebuild_image: &PathBuf, scripts_dir: &Path) {
    let mut target_dir = env::current_dir().expect("Get working dir failed");
    target_dir.push(scripts_dir);
    let temp_dir = "temp";
    let volumn_arg = format!("-v{}:/{}", target_dir.display().to_string(), temp_dir);
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
            temp_dir,
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

    let mut target_dir = PathBuf::from(repos_dir);
    target_dir.push(repo_name);

    if target_dir.exists() && run_git_checkout(&target_dir, &commit).is_ok() {
        return Ok(());
    }
    run_command("rm", vec!["-rf", &target_dir.display().to_string()]).expect("Run rm dir failed");
    fs::create_dir_all(&target_dir).expect("Create dir failed");
    run_git_clone(repo_url, is_recursive, &target_dir).expect("Run git clone failed");
    run_git_checkout(&target_dir, &commit).expect("Run git checkout failed");
    Ok(())
}

fn run_git_clone(repo_url: Url, is_recursive: bool, path: &Path) -> Result<()> {
    let path = path.display().to_string();
    let mut args = vec!["clone", repo_url.as_str(), path.as_str()];
    if is_recursive {
        args.push("--recursive");
    }
    run_command("git", args)
}

fn run_git_checkout(repo_relative_path: &Path, commit: &str) -> Result<()> {
    let repo_dir = repo_relative_path.display().to_string();
    run_command(
        "git",
        vec!["-C", &repo_dir, "checkout", commit],
    )?;
    run_command(
        "git",
        vec!["-C", &repo_dir, "submodule", "update", "--recursive"],
    )
}

fn build_godwoken_scripts(repos_dir: &Path, repo_name: &str) -> Result<()> {
    let mut target_dir = PathBuf::from(repos_dir);
    target_dir.push(repo_name);
    target_dir.push("c");
    run_command("make", vec!["-C", &target_dir.display().to_string()]).expect("Run make failed");

    target_dir.pop();
    run_command_in_dir(
        "capsule",
        vec!["build", "--release", "--debug-output"],
        &target_dir,
    )
    .expect("Run capsule build failed");

    Ok(())
}

fn build_godwoken_polyjuice(repos_dir: &Path, repo_name: &str) -> Result<()> {
    let mut target_dir = PathBuf::from(repos_dir);
    target_dir.push(repo_name);
    run_command(
        "make",
        vec!["-C", &target_dir.display().to_string(), "all-via-docker"],
    )
    .expect("Run make failed");
    Ok(())
}

fn build_clerkb(repos_dir: &Path, repo_name: &str) -> Result<()> {
    let mut target_dir = PathBuf::from(repos_dir);
    target_dir.push(repo_name);
    run_command("yarn", vec!["--cwd", &target_dir.display().to_string()]).expect("Run yarn failed");
    run_command(
        "make",
        vec!["-C", &target_dir.display().to_string(), "all-via-docker"],
    )
    .expect("Run make failed");
    Ok(())
}

fn run_command_in_dir<I, S>(bin: &str, args: I, target_dir: &Path) -> Result<()>
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
