use anyhow::{Context, Result, ensure};
#[cfg(any(unix, test))]
use std::fs;
use std::{
    env,
    path::Path,
    process::{Command, Output},
};

fn scheduler_environment(
    lookup: impl Fn(&str) -> Option<std::ffi::OsString>,
) -> Result<Vec<(String, String)>> {
    let dirs = directories::BaseDirs::new().context("cannot determine user directories")?;
    // Save resolved roots, not the shell environment; credentials never enter task files.
    #[cfg(windows)]
    let mut variables = vec![(
        "LOCALAPPDATA",
        lookup("LOCALAPPDATA").unwrap_or_else(|| dirs.data_local_dir().into()),
    )];
    #[cfg(not(windows))]
    let mut variables = vec![
        ("HOME", dirs.home_dir().into()),
        ("XDG_CONFIG_HOME", dirs.config_dir().into()),
        ("XDG_DATA_HOME", dirs.data_local_dir().into()),
    ];
    for name in ["CODEX_HOME", "HERMES_HOME"] {
        // Empty overrides preserve defaults even if the scheduler inherits other values.
        variables.push((name, lookup(name).unwrap_or_default()));
    }
    variables
        .into_iter()
        .map(|(name, value)| {
            let value = value
                .to_str()
                .with_context(|| format!("{name} must be valid Unicode"))?;
            let value = if name == "HERMES_HOME" {
                value.trim()
            } else {
                value
            };
            ensure!(
                !value.chars().any(char::is_control),
                "{name} contains control characters"
            );
            let path = if value.is_empty() {
                std::path::PathBuf::new()
            } else {
                // Relative integration homes must not depend on the task's working directory.
                std::path::absolute(value)?
            };
            Ok((
                name.to_owned(),
                path.to_str()
                    .context("Path must be valid Unicode")?
                    .to_owned(),
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_only_path_settings_and_resolves_relative_homes() {
        let environment = scheduler_environment(|name| match name {
            "CODEX_HOME" => Some("custom codex".into()),
            "HERMES_HOME" => Some("  custom hermes  ".into()),
            "API_KEY" => panic!("must not read credentials"),
            _ => None,
        })
        .unwrap();
        for (name, relative) in [
            ("CODEX_HOME", "custom codex"),
            ("HERMES_HOME", "custom hermes"),
        ] {
            let expected = std::path::absolute(relative).unwrap();
            assert!(environment.contains(&(name.into(), expected.to_str().unwrap().into())));
        }
        assert!(environment.iter().all(|(name, _)| {
            [
                "HOME",
                "XDG_CONFIG_HOME",
                "XDG_DATA_HOME",
                "LOCALAPPDATA",
                "CODEX_HOME",
                "HERMES_HOME",
            ]
            .contains(&name.as_str())
        }));
        let defaults = scheduler_environment(|_| None).unwrap();
        assert!(defaults.contains(&("CODEX_HOME".into(), String::new())));
        assert!(defaults.contains(&("HERMES_HOME".into(), String::new())));
        assert!(scheduler_environment(|_| Some("bad\npath".into())).is_err());
    }
}

pub(crate) fn install(executable: &Path) -> Result<()> {
    let executable = executable
        .to_str()
        .context("Executable path must be valid Unicode")?;
    ensure!(
        !executable.chars().any(char::is_control),
        "Executable path contains control characters"
    );
    let environment = scheduler_environment(|name| env::var_os(name))?;
    #[cfg(target_os = "windows")]
    return windows::install(executable, &environment);
    #[cfg(target_os = "macos")]
    return macos::install(executable, &environment);
    #[cfg(target_os = "linux")]
    return linux::install(executable, &environment);
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    anyhow::bail!("Automatic updates unsupported on this operating system");
}

pub(crate) fn uninstall() -> Result<()> {
    #[cfg(target_os = "windows")]
    return windows::uninstall();
    #[cfg(target_os = "macos")]
    return macos::uninstall();
    #[cfg(target_os = "linux")]
    return linux::uninstall();
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    anyhow::bail!("Automatic updates unsupported on this operating system");
}

fn checked(command: &mut Command) -> Result<Output> {
    let output = command
        .output()
        .with_context(|| format!("run {}", command.get_program().to_string_lossy()))?;
    ensure!(
        output.status.success(),
        "{} failed ({}): {} {}",
        command.get_program().to_string_lossy(),
        output.status,
        String::from_utf8_lossy(&output.stdout).trim(),
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(output)
}

#[cfg(any(unix, test))]
fn write_atomic(path: &Path, contents: &str) -> Result<()> {
    use std::io::Write;
    let mut temporary =
        tempfile::NamedTempFile::new_in(path.parent().context("Missing parent directory")?)?;
    temporary.write_all(contents.as_bytes())?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(path)
        .with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

#[cfg(any(target_os = "windows", test))]
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
mod windows {
    use super::*;

    fn quote(value: &str) -> String {
        format!("'{}'", value.replace('\'', "''"))
    }

    fn script(executable: Option<&str>, environment: &[(String, String)]) -> String {
        let mut script = String::from(
            "$ErrorActionPreference='Stop'; $sid=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value; $name='Auranion Auto Update '+$sid; ",
        );
        if let Some(executable) = executable {
            let assignments = environment
                .iter()
                .map(|(name, value)| format!("$env:{name}={}; ", quote(value)))
                .collect::<String>();
            let action = format!(
                "$ErrorActionPreference='Stop'; {}& {} update --background; exit $LASTEXITCODE",
                assignments,
                quote(executable)
            );
            script.push_str(&format!(r#"
$encoded=[Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes({}));
$action=New-ScheduledTaskAction -Execute "$PSHOME\powershell.exe" -Argument ('-NoProfile -NonInteractive -WindowStyle Hidden -EncodedCommand '+$encoded);
$principal=New-ScheduledTaskPrincipal -UserId $sid -LogonType Interactive -RunLevel Limited;
$daily=New-ScheduledTaskTrigger -Daily -At '00:00';
$login=New-ScheduledTaskTrigger -AtLogOn -User $sid;
$settings=New-ScheduledTaskSettingsSet -StartWhenAvailable -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -MultipleInstances IgnoreNew -ExecutionTimeLimit (New-TimeSpan -Minutes 15);
Register-ScheduledTask -TaskName $name -Action $action -Trigger @($daily,$login) -Principal $principal -Settings $settings -Description 'Daily Auranion auto-update' -Force | Out-Null;
"#, quote(&action)));
        } else {
            script.push_str("Get-ScheduledTask -TaskPath '\\' | Where-Object { $_.TaskName -eq $name } | Unregister-ScheduledTask -Confirm:$false;");
        }
        script
    }

    fn powershell(script: &str) -> Result<()> {
        let root = std::env::var_os("SystemRoot").context("SystemRoot is missing")?;
        let mut command =
            Command::new(Path::new(&root).join("System32/WindowsPowerShell/v1.0/powershell.exe"));
        command.args(["-NoProfile", "-NonInteractive", "-Command", script]);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        checked(&mut command)?;
        Ok(())
    }

    pub(super) fn install(executable: &str, environment: &[(String, String)]) -> Result<()> {
        powershell(&script(Some(executable), environment))
    }
    pub(super) fn uninstall() -> Result<()> {
        powershell(&script(None, &[]))
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn task_is_per_user_and_path_is_quoted() {
            let script = script(Some("C:\\User's Files\\auranion.exe"), &[]);
            assert!(script.contains("User''''s Files"));
            assert!(script.contains("-LogonType Interactive -RunLevel Limited"));
            assert!(script.contains("'Auranion Auto Update '+$sid"));
            assert!(script.contains("-WindowStyle Hidden"));
            assert!(script.contains("-Daily -At '00:00'"));
        }
        #[cfg(windows)]
        #[test]
        fn registration_script_runs_with_mocked_task_cmdlets() {
            let mocks = r#"
function New-ScheduledTaskAction {
    param($Execute,$Argument)
    $body=[Text.Encoding]::Unicode.GetString([Convert]::FromBase64String(($Argument -split ' ')[-1]));
    $invoke=$body.LastIndexOf('& ');
    if ($body.Substring($invoke) -cne "& 'C:\User''s Files\auranion.exe' update --background; exit `$LASTEXITCODE") { throw 'bad executable quoting' };
    $env:CODEX_HOME='wrong scheduler path';
    $env:HERMES_HOME='wrong scheduler path';
    & ([ScriptBlock]::Create($body.Substring(0,$invoke)));
    if ($env:CODEX_HOME -cne 'C:\path with ''quote''\$dollar%&') { throw 'lost setup path' };
    if ($env:HERMES_HOME) { throw 'inherited unwanted override' };
    @{}
}
function New-ScheduledTaskPrincipal { param($UserId,$LogonType,$RunLevel) if ($RunLevel -ne 'Limited') { throw 'elevated task' }; @{} }
function New-ScheduledTaskTrigger { param([switch]$Daily,$At,[switch]$AtLogOn,$User) @{} }
function New-ScheduledTaskSettingsSet { param([switch]$StartWhenAvailable,[switch]$AllowStartIfOnBatteries,[switch]$DontStopIfGoingOnBatteries,$MultipleInstances,$ExecutionTimeLimit) @{} }
function Register-ScheduledTask { param($TaskName,$Action,$Trigger,$Principal,$Settings,$Description,[switch]$Force) if ($Trigger.Count -ne 2) { throw 'missing trigger' } }
"#;
            powershell(&format!(
                "{mocks}\n{}",
                script(
                    Some("C:\\User's Files\\auranion.exe"),
                    &[
                        (
                            "CODEX_HOME".into(),
                            "C:\\path with 'quote'\\$dollar%&".into()
                        ),
                        ("HERMES_HOME".into(), String::new()),
                    ],
                )
            ))
            .unwrap();
        }
    }
}

#[cfg(any(target_os = "macos", test))]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod macos {
    use super::*;
    use directories::BaseDirs;
    const LABEL: &str = "com.auranion.config.updater";

    fn xml(value: &str) -> String {
        value
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&apos;")
    }

    fn plist(executable: &str, environment: &[(String, String)]) -> String {
        let executable = xml(executable);
        let environment = environment
            .iter()
            .map(|(name, value)| format!("<key>{}</key><string>{}</string>", xml(name), xml(value)))
            .collect::<String>();
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>{LABEL}</string>
<key>ProgramArguments</key><array><string>{executable}</string><string>update</string><string>--background</string></array>
<key>EnvironmentVariables</key><dict>{environment}</dict>
<key>RunAtLoad</key><true/>
<key>StartCalendarInterval</key><dict><key>Hour</key><integer>0</integer><key>Minute</key><integer>0</integer></dict>
<key>ProcessType</key><string>Background</string>
</dict></plist>
"#
        )
    }

    fn paths() -> Result<(std::path::PathBuf, String, String)> {
        let home = BaseDirs::new().context("cannot determine user directories")?;
        let output = checked(Command::new("/usr/bin/id").arg("-u"))?;
        let uid = String::from_utf8(output.stdout)?;
        let uid = uid.trim();
        ensure!(
            !uid.is_empty() && uid.bytes().all(|value| value.is_ascii_digit()),
            "Invalid user ID"
        );
        let domain = format!("gui/{uid}");
        Ok((
            home.home_dir()
                .join(format!("Library/LaunchAgents/{LABEL}.plist")),
            domain.clone(),
            format!("{domain}/{LABEL}"),
        ))
    }

    pub(super) fn install(executable: &str, environment: &[(String, String)]) -> Result<()> {
        let (path, domain, target) = paths()?;
        fs::create_dir_all(path.parent().unwrap())?;
        write_atomic(&path, &plist(executable, environment))?;
        // bootout of a missing job returns nonzero; bootstrap below checks registration.
        let _ = Command::new("/bin/launchctl")
            .args(["bootout", &target])
            .output();
        checked(Command::new("/bin/launchctl").args(["enable", &target]))?;
        checked(
            Command::new("/bin/launchctl")
                .args(["bootstrap", &domain])
                .arg(path),
        )?;
        Ok(())
    }

    pub(super) fn uninstall() -> Result<()> {
        let (path, _, target) = paths()?;
        // Missing or already-disabled labels are normal during repeated cleanup.
        let _ = Command::new("/bin/launchctl")
            .args(["disable", &target])
            .output();
        let _ = Command::new("/bin/launchctl")
            .args(["bootout", &target])
            .output();
        crate::update::remove_if_exists(&path)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn launch_agent_escapes_path_and_runs_daily_and_at_login() {
            let xml = plist(
                "/Users/A&B/auranion",
                &[("CODEX_HOME".into(), "/Users/A&B/custom".into())],
            );
            assert!(xml.contains("/Users/A&amp;B/auranion"));
            assert!(xml.contains("<key>CODEX_HOME</key><string>/Users/A&amp;B/custom</string>"));
            assert!(xml.contains("<key>RunAtLoad</key><true/>"));
            assert!(xml.contains("<key>Hour</key><integer>0</integer>"));
        }
    }
}

#[cfg(any(target_os = "linux", test))]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
mod linux {
    use super::*;
    use directories::BaseDirs;
    use std::{io::Write, process::Stdio};
    const SERVICE: &str = "auranion-update.service";
    const TIMER: &str = "auranion-update.timer";
    const BEGIN: &str = "# BEGIN AURANION AUTO-UPDATE";
    const END: &str = "# END AURANION AUTO-UPDATE";
    const TIMER_CONTENT: &str = "[Unit]\nDescription=Daily Auranion auto-update\n\n[Timer]\nOnCalendar=*-*-* 00:00:00\nPersistent=true\nRandomizedDelaySec=15m\n\n[Install]\nWantedBy=timers.target\n";

    fn unit_quote(value: &str) -> String {
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('%', "%%")
    }

    fn service(executable: &str, environment: &[(String, String)]) -> String {
        let executable = unit_quote(executable).replace('$', "$$");
        let environment = environment
            .iter()
            .map(|(name, value)| format!("Environment=\"{name}={}\"\n", unit_quote(value)))
            .collect::<String>();
        format!(
            "[Unit]\nDescription=Auranion daily auto-update\n\n[Service]\nType=oneshot\nTimeoutStartSec=15min\n{environment}ExecStart=\"{executable}\" update --background\n"
        )
    }

    fn systemd_available() -> bool {
        Command::new("systemctl")
            .args(["--user", "show-environment"])
            .output()
            .is_ok_and(|output| output.status.success())
    }

    fn unit_dir() -> Result<std::path::PathBuf> {
        Ok(BaseDirs::new()
            .context("cannot determine user directories")?
            .config_dir()
            .join("systemd/user"))
    }

    pub(super) fn install(executable: &str, environment: &[(String, String)]) -> Result<()> {
        if systemd_available() {
            let directory = unit_dir()?;
            fs::create_dir_all(&directory)?;
            write_atomic(&directory.join(SERVICE), &service(executable, environment))?;
            write_atomic(&directory.join(TIMER), TIMER_CONTENT)?;
            // Absolute paths also work when setup's XDG_CONFIG_HOME differs from the manager's.
            checked(
                Command::new("systemctl")
                    .args(["--user", "link"])
                    .arg(directory.join(SERVICE)),
            )?;
            checked(Command::new("systemctl").args(["--user", "daemon-reload"]))?;
            checked(
                Command::new("systemctl")
                    .args(["--user", "enable", "--now"])
                    .arg(directory.join(TIMER)),
            )?;
            return remove_cron();
        }
        let current = read_crontab()?.context(
            "No user systemd session or crontab; enable one, then run `auranion schedule enable`",
        )?;
        write_crontab(&cron(&current, executable, environment)?)
    }

    pub(super) fn uninstall() -> Result<()> {
        let directory = unit_dir()?;
        if directory.join(TIMER).try_exists()? {
            if systemd_available() {
                checked(
                    Command::new("systemctl").args(["--user", "disable", "--now", TIMER, SERVICE]),
                )?;
            }
            crate::update::remove_if_exists(&directory.join("timers.target.wants").join(TIMER))?;
            crate::update::remove_if_exists(&directory.join(TIMER))?;
            crate::update::remove_if_exists(&directory.join(SERVICE))?;
            if systemd_available() {
                checked(Command::new("systemctl").args(["--user", "daemon-reload"]))?;
            }
        }
        remove_cron()
    }

    fn shell_quote(value: &str) -> String {
        format!("'{}'", value.replace('\'', "'\\''"))
    }

    fn cron(current: &str, executable: &str, environment: &[(String, String)]) -> Result<String> {
        let mut next = remove_block(current)?;
        if !next.is_empty() && !next.ends_with('\n') {
            next.push('\n');
        }
        let environment = environment
            .iter()
            .map(|(name, value)| format!("{name}={} ", shell_quote(value)))
            .collect::<String>();
        let command = format!(
            "{environment}{} update --background >/dev/null 2>&1",
            shell_quote(executable)
        )
        .replace('%', "\\%");
        next.push_str(&format!("{BEGIN}\n0 0 * * * {command}\n{END}\n"));
        Ok(next)
    }

    fn read_crontab() -> Result<Option<String>> {
        let output = match Command::new("crontab")
            .arg("-l")
            .env("LC_ALL", "C")
            .output()
        {
            Ok(output) => output,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error).context("run crontab -l"),
        };
        if output.status.success() {
            return Ok(Some(String::from_utf8(output.stdout)?));
        }
        let error = String::from_utf8_lossy(&output.stderr);
        if output.status.code() == Some(1) && error.contains("no crontab for") {
            return Ok(Some(String::new()));
        }
        anyhow::bail!("crontab -l failed: {}", error.trim());
    }

    fn write_crontab(contents: &str) -> Result<()> {
        let mut child = Command::new("crontab")
            .arg("-")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .context("run crontab")?;
        let write = child
            .stdin
            .take()
            .context("open crontab input")?
            .write_all(contents.as_bytes());
        let output = child.wait_with_output()?;
        ensure!(
            output.status.success(),
            "crontab failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
        write.context("write crontab")
    }

    fn remove_cron() -> Result<()> {
        if let Some(current) = read_crontab()? {
            let next = remove_block(&current)?;
            if next != current {
                write_crontab(&next)?;
            }
        }
        Ok(())
    }

    fn remove_block(contents: &str) -> Result<String> {
        let mut result = String::new();
        let mut in_block = false;
        let mut found = false;
        for line in contents.split_inclusive('\n') {
            match line.trim_end_matches(['\r', '\n']) {
                BEGIN => {
                    ensure!(!found, "Duplicate Auranion crontab block");
                    in_block = true;
                    found = true;
                }
                END => {
                    ensure!(in_block, "Orphaned Auranion crontab marker");
                    in_block = false;
                }
                _ if !in_block => result.push_str(line),
                _ => (),
            }
        }
        ensure!(!in_block, "Incomplete Auranion crontab block");
        Ok(result)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn cron_is_idempotent_and_preserves_other_jobs() {
            let original = "# mine\r\n0 1 * * * backup\n";
            let environment = [("CODEX_HOME".into(), "/home/a b/custom's $value%".into())];
            let updated = cron(original, "/home/a b/it's 50%/auranion", &environment).unwrap();
            assert!(updated.contains("it'\\''s 50\\%"));
            assert!(updated.contains("CODEX_HOME='/home/a b/custom'\\''s $value\\%'"));
            assert_eq!(
                cron(&updated, "/home/a b/it's 50%/auranion", &environment).unwrap(),
                updated
            );
            assert_eq!(remove_block(&updated).unwrap(), original);
            assert!(remove_block(&format!("{BEGIN}\n")).is_err());
            assert!(remove_block(&format!("{END}\n")).is_err());
            assert!(remove_block(&format!("{BEGIN}\n{END}\n{BEGIN}\n{END}\n")).is_err());
        }
        #[test]
        fn systemd_quotes_percent_and_dollar_expansion() {
            let service = service(
                "/home/50%/$user/auranion",
                &[("CODEX_HOME".into(), "/home/50%/$user/custom".into())],
            );
            assert!(service.contains("\"/home/50%%/$$user/auranion\""));
            assert!(service.contains("Environment=\"CODEX_HOME=/home/50%%/$user/custom\""));
            assert!(TIMER_CONTENT.contains("Persistent=true"));
        }
    }
}
