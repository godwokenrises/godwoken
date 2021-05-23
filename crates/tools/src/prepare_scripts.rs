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

fn copy_scripts(_prebuild_image: &PathBuf, _scripts_dir: &Path) {}

fn generate_script_deploy_config(_output_path: &Path) {}

fn build_godwoken_scripts(repo_url: Url, root_dir: &Path, repo_name: &str) -> Result<()> {
    run_pull_code(repo_url, root_dir, repo_name)?;

    let mut target_dir = PathBuf::new();
    target_dir.push(root_dir);
    target_dir.push(repo_name);
    target_dir.push("c");

    let working_dir = env::current_dir().expect("Get working dir failed");
    env::set_current_dir(&target_dir).expect("Set target dir failed");
    run_command("make", vec!["-w"]).expect("Run make failed");

    env::set_current_dir("../").expect("Set target dir failed");
    run_command("capsule", vec!["build", "--release", "--debug-output"])
        .expect("Run capsule build failed");
    env::set_current_dir(&working_dir).expect("Set working dir failed");

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

    let commit = repo
        .fragment()
        .ok_or_else(|| anyhow::anyhow!("Invalid commit"))?
        .to_owned();
    repo.set_fragment(None);

    let mut target_dir = PathBuf::new();
    target_dir.push(root_dir);
    target_dir.push(repo_name);

    if target_dir.exists() && run_git_checkout(&target_dir, &commit).is_ok() {
        return Ok(());
    }
    run_command("rm", vec!["-rf", &target_dir.display().to_string()]).expect("Run rm dir failed");
    fs::create_dir_all(&target_dir).expect("Create dir failed");
    run_git_clone(repo, true, &target_dir).expect("Run git clone failed");
    run_git_checkout(&target_dir, &commit).expect("Run git checkout failed");
    Ok(())
}

fn run_git_clone(repo_url: Url, is_recursive: bool, path: &Path) -> Result<()> {
    let arg_recursive = if is_recursive { "--recursive" } else { "" };
    run_command(
        "git",
        vec![
            "clone",
            arg_recursive,
            repo_url.as_str(),
            &path.display().to_string(),
        ],
    )
}

fn run_git_checkout(repo_relative_path: &Path, commit: &str) -> Result<()> {
    let current_dir = env::current_dir().expect("Get current dir failed");
    env::set_current_dir(&repo_relative_path).expect("Set current dir failed");
    let result = run_command("git", vec!["checkout", commit]);
    env::set_current_dir(&current_dir).expect("Set current dir failed");
    result
}

pub fn run_command<I, S>(bin: &str, args: I) -> Result<()>
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
