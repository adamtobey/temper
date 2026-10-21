//! Bounded child processes and isolated candidate builds.

use anyhow::{Context, Result, bail, ensure};
use std::{
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub(crate) fn run(mut command: Command, cwd: &Path, seconds: u64) -> Result<String> {
    let output = tempfile::tempfile()?;
    let errors = tempfile::tempfile()?;
    command
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(output.try_clone()?)
        .stderr(errors.try_clone()?);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn().context("start Lean subprocess")?;
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let status = loop {
        if let Some(status) = child.try_wait()? {
            #[cfg(unix)]
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            break status;
        }
        if Instant::now() >= deadline {
            #[cfg(unix)]
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            #[cfg(not(unix))]
            child.kill()?;
            child.wait()?;
            bail!("Lean subprocess budget exhausted; result is inconclusive");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    use std::io::{Read, Seek};
    let read = |mut file: std::fs::File| -> Result<String> {
        ensure!(
            file.metadata()?.len() <= 4 * 1024 * 1024,
            "subprocess output budget exhausted"
        );
        file.rewind()?;
        let mut data = String::new();
        file.read_to_string(&mut data)?;
        Ok(data)
    };
    let stdout = read(output)?;
    let stderr = read(errors)?;
    ensure!(
        status.success(),
        "Lean subprocess failed ({status}): {stdout}\n{stderr}"
    );
    Ok(stdout)
}

pub(crate) fn command(
    root: &Path,
    job: &Path,
    program: &str,
    args: &[&str],
    isolated: bool,
) -> Result<Command> {
    let executable = root.join("bin").join(program);
    let mut command;
    if isolated {
        #[cfg(target_os = "macos")]
        {
            let quote = |p: &Path| -> Result<String> {
                let canonical = p.canonicalize()?;
                let p = canonical.as_path();
                let p = p.to_str().context("non-UTF8 sandbox path")?;
                ensure!(!p.contains(['"', '\\']), "invalid sandbox path");
                Ok(format!("\"{p}\""))
            };
            let profile = format!(
                "(version 1)(allow default)(deny network*)(deny file-write* (require-not (subpath {})))(deny file-read-data (require-all (subpath \"/Users\") (require-not (subpath {}))))(deny file-read-data (require-all (subpath \"/private/var/folders\") (require-not (subpath {}))))(deny file-read-data (require-all (subpath \"/private/tmp\") (require-not (subpath {}))))",
                quote(job)?,
                quote(root)?,
                quote(job)?,
                quote(job)?
            );
            let profile = if job.join("contract").exists() {
                format!(
                    "{profile}(deny file-write* (subpath {}))",
                    quote(&job.join("contract"))?
                )
            } else {
                profile
            };
            command = Command::new("/usr/bin/sandbox-exec");
            command.args(["-p", &profile]).arg(&executable);
        }
        #[cfg(target_os = "linux")]
        {
            command = Command::new("bwrap");
            command
                .args([
                    "--unshare-all",
                    "--die-with-parent",
                    "--new-session",
                    "--ro-bind",
                    "/usr",
                    "/usr",
                    "--ro-bind",
                    "/lib",
                    "/lib",
                    "--proc",
                    "/proc",
                    "--dev",
                    "/dev",
                    "--symlink",
                    "usr/bin",
                    "/bin",
                ])
                .arg("--ro-bind")
                .arg(root)
                .arg(root)
                .arg("--bind")
                .arg(job)
                .arg(job);
            if Path::new("/lib64").exists() {
                command.args(["--ro-bind", "/lib64", "/lib64"]);
            }
            if Path::new("/etc/ld.so.cache").exists() {
                command.args(["--ro-bind", "/etc/ld.so.cache", "/etc/ld.so.cache"]);
            }
            if job.join("contract").exists() {
                command
                    .arg("--ro-bind")
                    .arg(job.join("contract"))
                    .arg(job.join("contract"));
            }
            command.arg(executable);
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            bail!("Lean candidate isolation is not supported on this platform");
        }
    } else {
        command = Command::new(executable);
    }
    let search_path = std::env::join_paths([job.join("contract"), job.to_path_buf()])?;
    command
        .args(args)
        .env_clear()
        .env("PATH", root.join("bin"))
        .env("LEAN_PATH", search_path)
        .env("TMPDIR", job)
        .env("LC_ALL", "C");
    Ok(command)
}
