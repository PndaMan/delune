//! `delune setup`: ask the questions in a terminal wizard, then write everything a
//! working install needs — `config.toml`, the router setting, and a systemd unit —
//! and start it.

use std::io::IsTerminal as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use anyhow::{Context as _, Result, bail};
use delune_core::api::SharingSettings;
use delune_server::setup::{CheckResult, FileConfig, NavidromeFile, SetupRequest, SoulseekFile};
use delune_tui::setup::{Answers, Check, CheckFn, Part, Service};

const GREEN: &str = "\x1b[38;2;111;211;155m";
const ACCENT: &str = "\x1b[38;2;174;184;255m";
const MUTED: &str = "\x1b[38;2;139;145;168m";
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";

fn is_root() -> bool {
    std::env::var("USER").is_ok_and(|u| u == "root") || std::env::var_os("HOME").is_some_and(|h| h == "/root")
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from)
}

/// Where delune keeps its data by default: the system's place when run as root,
/// otherwise the user's own.
fn default_data_dir() -> PathBuf {
    if is_root() {
        return PathBuf::from("/var/lib/delune");
    }
    std::env::var_os("XDG_DATA_HOME").map_or_else(|| home().join(".local/share"), PathBuf::from).join("delune")
}

fn defaults(data_dir: Option<PathBuf>) -> Answers {
    let data_dir = data_dir.unwrap_or_else(default_data_dir);
    // Start from whatever an earlier run saved.
    let saved = FileConfig::load(&FileConfig::path(&data_dir)).unwrap_or_default();
    let (navidrome, soulseek) = (saved.navidrome.unwrap_or_default(), saved.soulseek.unwrap_or_default());
    Answers {
        data_dir: data_dir.display().to_string(),
        library_dir: saved.library_dir.map(|p| p.display().to_string()).unwrap_or_default(),
        navidrome_url: if navidrome.url.is_empty() { "http://localhost:4533".into() } else { navidrome.url },
        navidrome_username: navidrome.username,
        navidrome_password: navidrome.password,
        soulseek_username: soulseek.username,
        soulseek_password: soulseek.password,
        soulseek_port: soulseek.port.unwrap_or(2234).to_string(),
        bind: "0.0.0.0:7474".into(),
        upnp: false,
        service: if is_root() { Service::System } else { Service::User },
        start_now: true,
    }
}

fn from_result(result: Option<CheckResult>, skipped: &str) -> Check {
    result.map_or_else(|| Check { ok: true, message: skipped.to_owned() }, |r| Check { ok: r.ok, message: r.message })
}

fn request(answers: &Answers) -> SetupRequest {
    SetupRequest {
        library_dir: Some(answers.library_dir.trim().to_owned()),
        navidrome: Some(NavidromeFile {
            url: answers.navidrome_url.trim().to_owned(),
            username: answers.navidrome_username.trim().to_owned(),
            password: answers.navidrome_password.clone(),
        }),
        soulseek: Some(SoulseekFile {
            username: answers.soulseek_username.trim().to_owned(),
            password: answers.soulseek_password.clone(),
            port: answers.soulseek_port.trim().parse().ok(),
        }),
    }
}

async fn check(part: Part, answers: Answers) -> Check {
    match part {
        Part::Folders => {
            let data = PathBuf::from(answers.data_dir.trim());
            if !data.is_absolute() {
                return Check { ok: false, message: "Use a full path for delune's data, starting with /.".into() };
            }
            if let Err(e) = std::fs::create_dir_all(&data) {
                return Check { ok: false, message: format!("Can't create {}: {e}.", data.display()) };
            }
            let only_library = SetupRequest { library_dir: request(&answers).library_dir, ..SetupRequest::default() };
            let found = delune_server::setup::check_offline(&only_library).await;
            from_result(found.library, "")
        }
        Part::Navidrome => {
            if answers.navidrome_url.trim().is_empty() {
                return Check {
                    ok: true,
                    message: "Skipping Navidrome: anyone who can reach delune can use it.".into(),
                };
            }
            let only = SetupRequest { navidrome: request(&answers).navidrome, ..SetupRequest::default() };
            from_result(delune_server::setup::check_offline(&only).await.navidrome, "")
        }
        Part::Soulseek => {
            if answers.soulseek_username.trim().is_empty() {
                return Check { ok: true, message: "Skipping Soulseek for now; search needs it.".into() };
            }
            if answers.soulseek_port.trim().parse::<u16>().is_err() {
                return Check { ok: false, message: "The port is a number from 1 to 65535.".into() };
            }
            let only = SetupRequest { soulseek: request(&answers).soulseek, ..SetupRequest::default() };
            from_result(delune_server::setup::check_offline(&only).await.soulseek, "")
        }
    }
}

/// Run the wizard and apply what it gathered.
///
/// # Errors
///
/// Without a terminal, or when the files can't be written.
pub fn run(data_dir: Option<PathBuf>) -> Result<()> {
    if !std::io::stdout().is_terminal() {
        bail!("delune setup needs a terminal. Run it directly, or configure with DELUNE_* variables instead.");
    }
    let checker: CheckFn = Arc::new(|part, answers| Box::pin(check(part, answers)));
    let defaults = defaults(data_dir);
    let answers = tokio::task::block_in_place(|| delune_tui::setup::run(defaults, checker))?;
    let Some(answers) = answers else {
        println!("{MUTED}Setup cancelled; nothing was changed.{RESET}");
        return Ok(());
    };
    apply(&answers)
}

fn done(message: impl AsRef<str>) {
    println!("  {GREEN}✓{RESET} {}", message.as_ref());
}

fn apply(answers: &Answers) -> Result<()> {
    println!("\n{ACCENT}☾{RESET} {BOLD}Setting up delune{RESET}\n");
    let data_dir = PathBuf::from(answers.data_dir.trim());
    std::fs::create_dir_all(&data_dir).with_context(|| format!("creating {}", data_dir.display()))?;

    let request = request(answers);
    let config = FileConfig {
        library_dir: request.library_dir.filter(|d| !d.is_empty()).map(PathBuf::from),
        soulseek: request.soulseek.filter(|s| !s.username.is_empty()),
        navidrome: request.navidrome.filter(|n| !n.url.is_empty()),
    };
    let config_path = FileConfig::path(&data_dir);
    config.save(&config_path).with_context(|| format!("writing {}", config_path.display()))?;
    done(format!("Saved your connections to {}", config_path.display()));

    let db = delune_server::store::Database::open(&data_dir).context("opening delune's database")?;
    let mut sharing: SharingSettings = db.load("sharing").unwrap_or_default();
    sharing.upnp = answers.upnp;
    db.save("sharing", &sharing);
    if answers.upnp {
        done("delune will ask your router to forward the Soulseek port");
    }

    let started = match answers.service {
        Service::None => false,
        Service::User => install_user_service(answers, &data_dir)?,
        Service::System => install_system_service(answers, &data_dir)?,
    };

    let port = answers.bind.rsplit(':').next().unwrap_or("7474");
    let host = lan_address().unwrap_or_else(|| "localhost".to_owned());
    println!();
    if started {
        println!("  {BOLD}delune is running.{RESET} Open {ACCENT}http://{host}:{port}{RESET}");
    } else {
        println!(
            "  {BOLD}Ready.{RESET} Start it with {ACCENT}DELUNE_DATA_DIR={} DELUNE_BIND={} delune serve{RESET}",
            data_dir.display(),
            answers.bind
        );
        println!("  then open {ACCENT}http://{host}:{port}{RESET}");
    }
    println!("  {MUTED}Everything here can be changed later under Settings.{RESET}\n");
    Ok(())
}

fn unit(answers: &Answers, data_dir: &Path, system: Option<&str>) -> Result<String> {
    let exe = std::env::current_exe().context("finding the delune binary")?;
    let user = system.map(|u| format!("User={u}\n")).unwrap_or_default();
    let target = if system.is_some() { "multi-user.target" } else { "default.target" };
    Ok(format!(
        r#"[Unit]
Description=delune: find music on Soulseek, review it, add it to your library
Documentation=https://github.com/PndaMan/delune
Wants=network-online.target
After=network-online.target

[Service]
{user}ExecStart="{exe}" serve
Environment="DELUNE_DATA_DIR={data}"
Environment="DELUNE_BIND={bind}"
Restart=on-failure
RestartSec=5
UMask=0002

[Install]
WantedBy={target}
"#,
        exe = systemd_escape(&exe.display().to_string()),
        data = systemd_escape(&data_dir.display().to_string()),
        bind = systemd_escape(answers.bind.trim()),
    ))
}

/// A value for inside a quoted unit-file setting: `%` and quotes would otherwise be read.
fn systemd_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"").replace('%', "%%")
}

fn systemctl(args: &[&str]) -> bool {
    Command::new("systemctl").args(args).status().is_ok_and(|s| s.success())
}

fn install_user_service(answers: &Answers, data_dir: &Path) -> Result<bool> {
    let dir =
        std::env::var_os("XDG_CONFIG_HOME").map_or_else(|| home().join(".config"), PathBuf::from).join("systemd/user");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("delune.service");
    std::fs::write(&path, unit(answers, data_dir, None)?).with_context(|| format!("writing {}", path.display()))?;
    done(format!("Wrote a service to {}", path.display()));
    if !answers.start_now {
        println!("    {MUTED}Start it with: systemctl --user enable --now delune{RESET}");
        return Ok(false);
    }
    if !(systemctl(&["--user", "daemon-reload"]) && systemctl(&["--user", "enable", "--now", "delune"])) {
        println!("    {MUTED}systemd didn't start it; try: systemctl --user enable --now delune{RESET}");
        return Ok(false);
    }
    // Restart in case an earlier delune was already running with old settings.
    systemctl(&["--user", "restart", "delune"]);
    done("Started delune, and it starts again when you log in");
    println!("    {MUTED}To keep it running while you're logged out: loginctl enable-linger {}{RESET}", whoami());
    Ok(true)
}

fn whoami() -> String {
    std::env::var("USER").unwrap_or_else(|_| "$USER".to_owned())
}

fn install_system_service(answers: &Answers, data_dir: &Path) -> Result<bool> {
    // Under sudo, run as the person who asked rather than as root.
    let account = std::env::var("SUDO_USER").unwrap_or_else(|_| "root".to_owned());
    let text = unit(answers, data_dir, Some(&account))?;
    if !is_root() {
        let path = data_dir.join("delune.service");
        std::fs::write(&path, text)?;
        done(format!("Wrote a system service to {}", path.display()));
        println!("    {MUTED}Install it with root:{RESET}");
        println!("    sudo cp {} /etc/systemd/system/ && sudo systemctl enable --now delune", path.display());
        return Ok(false);
    }
    let path = Path::new("/etc/systemd/system/delune.service");
    std::fs::write(path, text).context("writing /etc/systemd/system/delune.service")?;
    if account != "root" {
        // Only what setup made, never a whole tree someone may have mistyped.
        let mut owned = vec![data_dir.to_path_buf()];
        owned.extend(["config.toml", "delune.db", "delune.db-wal", "delune.db-shm"].iter().map(|f| data_dir.join(f)));
        for path in owned.iter().filter(|p| p.exists()) {
            let _ = Command::new("chown").arg(&account).arg(path).status();
        }
    }
    done(format!("Wrote {} (runs as {account})", path.display()));
    if answers.start_now && systemctl(&["daemon-reload"]) && systemctl(&["enable", "--now", "delune"]) {
        systemctl(&["restart", "delune"]);
        done("Started delune, and it starts with the machine");
        return Ok(true);
    }
    println!("    {MUTED}Start it with: systemctl enable --now delune{RESET}");
    Ok(false)
}

/// This machine's address on the local network, for the link to open.
fn lan_address() -> Option<String> {
    // Connecting a UDP socket sends nothing; it only picks the outgoing interface.
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.2.1:9").ok()?;
    Some(socket.local_addr().ok()?.ip().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_run_this_binary_with_the_chosen_settings() {
        let answers = Answers { bind: "0.0.0.0:8080".into(), ..Answers::default() };
        let text = unit(&answers, Path::new("/srv/delune"), None).unwrap();
        assert!(text.contains("Environment=\"DELUNE_DATA_DIR=/srv/delune\""));
        assert!(text.contains("Environment=\"DELUNE_BIND=0.0.0.0:8080\""));
        assert!(text.contains("\" serve\n"));
        let odd = unit(&answers, Path::new("/home/a/My 100% Data"), None).unwrap();
        assert!(odd.contains("Environment=\"DELUNE_DATA_DIR=/home/a/My 100%% Data\""));
        assert!(text.contains("WantedBy=default.target"));
        assert!(!text.contains("User="));
        let system = unit(&answers, Path::new("/srv/delune"), Some("aidan")).unwrap();
        assert!(system.contains("User=aidan\n") && system.contains("WantedBy=multi-user.target"));
    }

    #[test]
    fn empty_answers_leave_connections_out() {
        let answers = Answers { data_dir: "/d".into(), soulseek_port: "2235".into(), ..Answers::default() };
        let r = request(&answers);
        assert_eq!(r.soulseek.as_ref().unwrap().port, Some(2235));
        assert!(r.soulseek.unwrap().username.is_empty());
        assert!(r.navidrome.unwrap().url.is_empty());
    }

    #[tokio::test]
    async fn folders_must_be_real_and_absolute() {
        let relative = Answers { data_dir: "relative".into(), ..Answers::default() };
        assert!(!check(Part::Folders, relative).await.ok);
        let dir = std::env::temp_dir().join(format!("delune-setup-{}", std::process::id()));
        let fine = Answers {
            data_dir: dir.display().to_string(),
            library_dir: dir.display().to_string(),
            ..Answers::default()
        };
        assert!(check(Part::Folders, fine).await.ok);
        let missing =
            Answers { data_dir: dir.display().to_string(), library_dir: "/no/such/music".into(), ..Answers::default() };
        assert!(!check(Part::Folders, missing).await.ok);
        let skipped = Answers { data_dir: dir.display().to_string(), ..Answers::default() };
        assert!(check(Part::Navidrome, skipped).await.ok, "no Navidrome is allowed");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
