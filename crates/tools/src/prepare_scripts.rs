use anyhow::Result;
use clap::arg_enum;
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
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
    tmp_dir: &Path,
    scripts_dir: &Path,
    _output_path: &Path,
) -> Result<()> {
    let input = fs::read_to_string(input_path)?;
    let scripts_build_config: ScriptsBuildConfig = serde_json::from_str(input.as_str())?;
    match mode {
        ScriptsBuildMode::Build => build_scripts(scripts_build_config.repos, tmp_dir, scripts_dir)?,
        ScriptsBuildMode::Copy => copy_scripts(&scripts_build_config.prebuild_image, scripts_dir),
    }
    generate_script_deploy_config(_output_path);
    Ok(())
}

fn build_scripts(repos: Repos, root_dir: &Path, _scripts_dir: &Path) -> Result<()> {
    build_godwoken_scripts(repos.godwoken_scripts, root_dir, GODWOKEN_SCRIPTS)?;
    build_godwoken_polyjuice(repos.godwoken_polyjuice, root_dir, GODWOKEN_POLYJUICE)?;
    build_clerkb(repos.clerkb, root_dir, CLERKB)?;
    Ok(())
}

fn build_godwoken_scripts(repo_url: Url, root_dir: &Path, repo_name: &str) -> Result<()> {
    // run_pull_code(repo_url, root_dir, repo_name)?;

    let current_dir = env::current_dir()?;

    let mut path = PathBuf::new();
    path.push(root_dir);
    path.push(repo_name);
    path.push("c");
    env::set_current_dir(&path)?;
    let status = Command::new("make").status()?;
    log::debug!("make: {}", status);

    env::set_current_dir("../")?;
    let status = Command::new("capsule")
        .arg("build")
        .arg("--release")
        .arg("--debug-output")
        .status()?;
    log::debug!("capsule build: {}", status);

    env::set_current_dir(&current_dir)?;

    Ok(())
}

fn build_godwoken_polyjuice(repo_url: Url, root_dir: &Path, repo_name: &str) -> Result<()> {
    run_pull_code(repo_url, root_dir, repo_name)?;
    Ok(())
}

fn build_clerkb(repo_url: Url, root_dir: &Path, repo_name: &str) -> Result<()> {
    run_pull_code(repo_url, root_dir, repo_name)?;
    Ok(())
}

fn run_pull_code(mut repo: Url, root_dir: &Path, repo_name: &str) -> Result<()> {
    log::info!("Pull code of {} ...", repo_name);

    let mut path = PathBuf::new();
    path.push(root_dir);
    path.push(repo_name);
    let status = Command::new("rm")
        .arg("-rf")
        .arg(path.display().to_string())
        .status()?;
    fs::create_dir_all(&path)?;
    log::debug!("Clear repo: {}", status);

    let commit = repo
        .fragment()
        .ok_or_else(|| anyhow::anyhow!("Invalid commit"))?
        .to_owned();
    repo.set_fragment(None);

    let status = Command::new("git")
        .arg("clone")
        .arg("--recursive")
        .arg(repo.as_str())
        .arg(path.display().to_string())
        .status()?;
    log::debug!("git clone repo: {}", status);

    let current_dir = env::current_dir()?;
    env::set_current_dir(&path)?;
    let status = Command::new("git").arg("checkout").arg(commit).status()?;
    log::debug!("git checkout commit: {}", status);
    env::set_current_dir(&current_dir)?;

    Ok(())
}

fn copy_scripts(_prebuild_image: &PathBuf, _scripts_dir: &Path) {}

fn generate_script_deploy_config(_output_path: &Path) {}
