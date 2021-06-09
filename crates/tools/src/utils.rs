use anyhow::Result;
use std::path::{Path, PathBuf};
use std::{env, ffi::OsStr, process::Command};

pub fn run_in_dir<I, S>(bin: &str, args: I, target_dir: &str) -> Result<()>
where
    I: IntoIterator<Item = S> + std::fmt::Debug,
    S: AsRef<OsStr>,
{
    let working_dir = env::current_dir().expect("get working dir");
    env::set_current_dir(&target_dir).expect("set target dir");
    let result = run(bin, args);
    env::set_current_dir(&working_dir).expect("set working dir");
    result
}

pub fn run<I, S>(bin: &str, args: I) -> Result<()>
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

pub fn run_in_output_mode<I, S>(bin: &str, args: I) -> Result<(String, String), String>
where
    I: IntoIterator<Item = S> + std::fmt::Debug,
    S: AsRef<OsStr>,
{
    log::info!("[Execute]: {} {:?}", bin, args);
    let init_output = Command::new(bin.to_owned())
        .env("RUST_BACKTRACE", "full")
        .args(args)
        .output()
        .expect("Run command failed");

    if !init_output.status.success() {
        Err(format!(
            "{}",
            String::from_utf8_lossy(init_output.stderr.as_slice())
        ))
    } else {
        let stdout = String::from_utf8_lossy(init_output.stdout.as_slice()).to_string();
        let stderr = String::from_utf8_lossy(init_output.stderr.as_slice()).to_string();
        log::debug!("stdout: {}", stdout);
        log::debug!("stderr: {}", stderr);
        Ok((stdout, stderr))
    }
}

pub fn make_path<P: AsRef<Path>>(parent_dir_path: &Path, paths: Vec<P>) -> PathBuf {
    let mut target = PathBuf::from(parent_dir_path);
    for p in paths {
        target.push(p);
    }
    target
}
