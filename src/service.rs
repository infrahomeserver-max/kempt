//! Everything that lets Kempt keep working with no browser open: the single-instance file, desktop notifications and
//! sound for reminders, start-at-login, the own-window launcher and (on Linux) the tray icon.
//! All of it is best effort: if a helper program is missing, Kempt carries on without it.

use crate::plat;
use anyhow::{Result, bail};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

// ---------------------------------------------------------------- one running copy

pub struct Instance {
    pub port: u16,
    pub token: String,
}

fn instance_path() -> PathBuf {
    plat::config_dir().join("instance.json")
}

pub fn write_instance(port: u16, token: &str) {
    let p = instance_path();
    let _ = std::fs::create_dir_all(p.parent().unwrap());
    let _ = std::fs::write(&p, serde_json::json!({"port": port, "token": token, "pid": std::process::id()}).to_string());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600));
    }
}

pub fn clear_instance() {
    let _ = std::fs::remove_file(instance_path());
}

fn alive(port: u16) -> bool {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let Ok(mut s) = TcpStream::connect_timeout(&addr, Duration::from_millis(400)) else { return false };
    let _ = s.set_read_timeout(Some(Duration::from_millis(800)));
    let _ = s.set_write_timeout(Some(Duration::from_millis(800)));
    if write!(s, "GET /ping HTTP/1.0\r\nHost: 127.0.0.1:{port}\r\n\r\n").is_err() {
        return false;
    }
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    out.ends_with("kempt")
}

/// The Kempt that is already running for this user, if any.
pub fn running_instance() -> Option<Instance> {
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(instance_path()).ok()?).ok()?;
    let port = v["port"].as_u64()? as u16;
    let token = v["token"].as_str()?.to_string();
    if alive(port) { Some(Instance { port, token }) } else { None }
}

/// Ask the running copy to stop (`kempt stop`).
pub fn stop_running() -> Result<bool> {
    let Some(i) = running_instance() else { return Ok(false) };
    let mut s = TcpStream::connect_timeout(&SocketAddr::from(([127, 0, 0, 1], i.port)), Duration::from_secs(1))?;
    let body = "{}";
    write!(s, "POST /api/service/quit HTTP/1.0\r\nHost: 127.0.0.1:{}\r\nX-Token: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}", i.port, i.token, body.len(), body)?;
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    Ok(true)
}

pub fn pct(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~/".contains(&b) { o.push(b as char) } else { o.push_str(&format!("%{b:02X}")) }
    }
    o
}

// ---------------------------------------------------------------- opening the interface

fn spawn_quiet(mut c: Command) -> bool {
    c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    match c.spawn() {
        Ok(mut ch) => {
            std::thread::spawn(move || {
                let _ = ch.wait();
            });
            true
        }
        Err(_) => false,
    }
}

/// Browsers Kempt can be told to use: (setting value, Chromium-based?, Linux commands, macOS app, Windows paths under Program Files / LocalAppData)
const BROWSERS: &[(&str, bool, &[&str], &str, &str)] = &[
    ("chrome", true, &["google-chrome", "google-chrome-stable"], "Google Chrome", "Google/Chrome/Application/chrome.exe"),
    ("edge", true, &["microsoft-edge", "microsoft-edge-stable"], "Microsoft Edge", "Microsoft/Edge/Application/msedge.exe"),
    ("brave", true, &["brave-browser", "brave"], "Brave Browser", "BraveSoftware/Brave-Browser/Application/brave.exe"),
    ("chromium", true, &["chromium", "chromium-browser"], "Chromium", "Chromium/Application/chrome.exe"),
    ("vivaldi", true, &["vivaldi", "vivaldi-stable"], "Vivaldi", "Vivaldi/Application/vivaldi.exe"),
    ("firefox", false, &["firefox"], "Firefox", "Mozilla Firefox/firefox.exe"),
    ("safari", false, &[], "Safari", ""),
];

/// Ids of the browsers found on this computer (in the order of `BROWSERS`).
pub fn installed_browsers() -> Vec<&'static str> {
    BROWSERS.iter().filter(|b| find_browser(b.0, false).is_some()).map(|b| b.0).collect()
}

fn chosen_browser() -> String {
    crate::state::Store::new().get()["browser"].as_str().unwrap_or("").to_string()
}

/// Find an installed browser: (is Chromium-based, how to start it with a URL).
fn find_browser(want: &str, chromium_only: bool) -> Option<(bool, Command)> {
    for (id, chromium, bins, app, win) in BROWSERS {
        if (!want.is_empty() && *id != want) || (chromium_only && !chromium) {
            continue;
        }
        if cfg!(target_os = "macos") {
            if std::path::Path::new(&format!("/Applications/{app}.app")).exists() {
                let mut c = Command::new("open");
                c.args(["-na", app, "--args"]);
                return Some((*chromium, c));
            }
        } else if cfg!(windows) {
            for var in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
                if let Some(b) = std::env::var_os(var) {
                    let p = PathBuf::from(b).join(win);
                    if !win.is_empty() && p.is_file() {
                        return Some((*chromium, Command::new(p)));
                    }
                }
            }
        } else {
            for n in *bins {
                if let Some(p) = crate::external::find_bin(n) {
                    return Some((*chromium, Command::new(p)));
                }
            }
        }
    }
    None
}

/// Open `url` in a Chromium-based browser as an app window (its own window and taskbar entry, no tabs or address bar).
pub fn open_app_window(url: &str) -> bool {
    let want = chosen_browser();
    let want = if BROWSERS.iter().any(|b| b.0 == want && b.1) { want } else { String::new() };
    if let Some((_, mut c)) = find_browser(&want, true) {
        c.arg(format!("--app={url}"));
        if cfg!(target_os = "linux") {
            c.arg("--class=Kempt");   // groups the window under Kempt's own icon in the dock (matches StartupWMClass in kempt.desktop)
        }
        return spawn_quiet(c);
    }
    false
}

/// Open the interface the way the user prefers: own window if asked for (and a Chromium browser exists), else the browser chosen in
/// Settings, else the system default.
pub fn open_ui(url: &str, own_window: bool) {
    if own_window && open_app_window(url) {
        return;
    }
    let want = chosen_browser();
    if !want.is_empty() {
        if let Some((_, mut c)) = find_browser(&want, false) {
            c.arg(url);
            if spawn_quiet(c) {
                return;
            }
        }
    }
    plat::open_browser(url);
}

// ---------------------------------------------------------------- desktop notification and sound

fn override_cmd(var: &str) -> Option<String> {
    std::env::var(var).ok().filter(|v| !v.is_empty())
}

pub fn notify_available() -> bool {
    override_cmd("KEMPT_NOTIFY_CMD").is_some()
        || if cfg!(windows) || cfg!(target_os = "macos") { true } else { crate::external::find_bin("notify-send").is_some() }
}

pub fn sound_available() -> bool {
    override_cmd("KEMPT_SOUND_CMD").is_some() || player().is_some()
}

fn player() -> Option<(PathBuf, Vec<&'static str>)> {
    if cfg!(windows) {
        return crate::external::find_bin("powershell").map(|p| (p, vec![]));
    }
    if cfg!(target_os = "macos") {
        return Some((PathBuf::from("/usr/bin/afplay"), vec![]));
    }
    for (n, a) in [("paplay", vec![]), ("pw-play", vec![]), ("aplay", vec!["-q"]), ("play", vec!["-q"]), ("ffplay", vec!["-nodisp", "-autoexit", "-loglevel", "quiet"])] {
        if let Some(p) = crate::external::find_bin(n) {
            return Some((p, a));
        }
    }
    None
}

pub fn notify(title: &str, body: &str) -> bool {
    if let Some(cmd) = override_cmd("KEMPT_NOTIFY_CMD") {
        let mut c = Command::new(cmd);
        c.arg(title).arg(body);
        return spawn_quiet(c);
    }
    if cfg!(windows) {
        let script = "[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] > $null; \
            $t = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent([Windows.UI.Notifications.ToastTemplateType]::ToastText02); \
            $n = $t.GetElementsByTagName('text'); $n.Item(0).AppendChild($t.CreateTextNode($env:KEMPT_T)) > $null; $n.Item(1).AppendChild($t.CreateTextNode($env:KEMPT_B)) > $null; \
            $id = '{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\\WindowsPowerShell\\v1.0\\powershell.exe'; \
            [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier($id).Show([Windows.UI.Notifications.ToastNotification]::new($t))";
        let mut c = Command::new("powershell");
        c.args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-Command", script]).env("KEMPT_T", title).env("KEMPT_B", body);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x0800_0000);
        }
        return spawn_quiet(c);
    }
    if cfg!(target_os = "macos") {
        let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
        let mut c = Command::new("osascript");
        c.args(["-e", &format!("display notification \"{}\" with title \"{}\"", esc(body), esc(title))]);
        return spawn_quiet(c);
    }
    let Some(bin) = crate::external::find_bin("notify-send") else { return false };
    let mut c = Command::new(bin);
    c.args(["--app-name=Kempt", "--icon=kempt", "--urgency=normal", title, body]);
    spawn_quiet(c)
}

/// Three short tones as a 16-bit mono WAV (no sound file ships with Kempt).
fn alert_wav() -> Vec<u8> {
    let rate = 22050u32;
    let mut pcm: Vec<i16> = vec![];
    for (f, ms) in [(880.0f32, 180), (880.0, 180), (1175.0, 260)] {
        let n = (rate as usize * ms) / 1000;
        for i in 0..n {
            let t = i as f32 / rate as f32;
            let env = (i.min(n - i) as f32 / (rate as f32 * 0.02)).min(1.0);
            pcm.push((env * 0.35 * (2.0 * std::f32::consts::PI * f * t).sin() * i16::MAX as f32) as i16);
        }
        pcm.extend(std::iter::repeat_n(0, rate as usize * 90 / 1000));
    }
    let data_len = (pcm.len() * 2) as u32;
    let mut w = Vec::with_capacity(44 + data_len as usize);
    w.extend(b"RIFF");
    w.extend((36 + data_len).to_le_bytes());
    w.extend(b"WAVEfmt ");
    w.extend(16u32.to_le_bytes());
    w.extend(1u16.to_le_bytes());
    w.extend(1u16.to_le_bytes());
    w.extend(rate.to_le_bytes());
    w.extend((rate * 2).to_le_bytes());
    w.extend(2u16.to_le_bytes());
    w.extend(16u16.to_le_bytes());
    w.extend(b"data");
    w.extend(data_len.to_le_bytes());
    for s in pcm {
        w.extend(s.to_le_bytes());
    }
    w
}

pub fn play_alert() -> bool {
    let wav = plat::config_dir().join("alert.wav");
    if !wav.exists() {
        let _ = std::fs::create_dir_all(wav.parent().unwrap());
        if std::fs::write(&wav, alert_wav()).is_err() {
            return false;
        }
    }
    if let Some(cmd) = override_cmd("KEMPT_SOUND_CMD") {
        let mut c = Command::new(cmd);
        c.arg(&wav);
        return spawn_quiet(c);
    }
    let Some((bin, args)) = player() else { return false };
    let mut c = Command::new(bin);
    if cfg!(windows) {
        c.args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-Command", &format!("(New-Object Media.SoundPlayer '{}').PlaySync()", wav.display())]);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x0800_0000);
        }
    } else {
        c.args(args).arg(&wav);
    }
    spawn_quiet(c)
}

// ---------------------------------------------------------------- start at login

fn exe() -> Result<PathBuf> {
    Ok(std::env::current_exe()?)
}

fn linux_autostart_file() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()).map(PathBuf::from).unwrap_or_else(|| plat::home_dir().join(".config")).join("autostart").join("kempt.desktop")
}

fn mac_agent_file() -> PathBuf {
    plat::home_dir().join("Library").join("LaunchAgents").join("io.kempt.agent.plist")
}

pub fn autostart_on() -> bool {
    if cfg!(windows) {
        Command::new("reg").args(["query", r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run", "/v", "Kempt"]).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
    } else if cfg!(target_os = "macos") {
        mac_agent_file().exists()
    } else {
        linux_autostart_file().exists()
    }
}

pub fn set_autostart(on: bool) -> Result<()> {
    if cfg!(windows) {
        let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
        let st = if on {
            Command::new("reg").args(["add", key, "/v", "Kempt", "/t", "REG_SZ", "/d", &format!("\"{}\" gui --background --detach", exe()?.display()), "/f"]).stdout(Stdio::null()).stderr(Stdio::null()).status()?
        } else {
            let _ = Command::new("reg").args(["delete", key, "/v", "Kempt", "/f"]).stdout(Stdio::null()).stderr(Stdio::null()).status();
            return Ok(());
        };
        if !st.success() {
            bail!("could not write the Windows startup entry");
        }
    } else if cfg!(target_os = "macos") {
        let f = mac_agent_file();
        if on {
            std::fs::create_dir_all(f.parent().unwrap())?;
            let e = exe()?.display().to_string();
            std::fs::write(&f, format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict>\n<key>Label</key><string>io.kempt.agent</string>\n<key>ProgramArguments</key><array><string>{e}</string><string>gui</string><string>--background</string></array>\n<key>RunAtLoad</key><true/>\n</dict></plist>\n"))?;
        } else {
            let _ = std::fs::remove_file(&f);
        }
    } else {
        let f = linux_autostart_file();
        if on {
            std::fs::create_dir_all(f.parent().unwrap())?;
            std::fs::write(&f, format!("[Desktop Entry]\nType=Application\nName=Kempt (reminders)\nComment=Keeps Kempt reminders and the tray icon running\nExec=\"{}\" gui --background\nIcon=kempt\nTerminal=false\nNoDisplay=true\nX-GNOME-Autostart-enabled=true\n", exe()?.display()))?;
        } else {
            let _ = std::fs::remove_file(&f);
        }
    }
    Ok(())
}

/// Start a copy of ourselves in the background, detached from this terminal, and return.
pub fn spawn_detached() -> Result<()> {
    let mut c = Command::new(exe()?);
    c.args(["gui", "--background"]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000 | 0x0000_0008);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        c.process_group(0);
    }
    c.spawn()?;
    Ok(())
}

// ---------------------------------------------------------------- tray icon (Linux: StatusNotifierItem)

/// 32x32 ARGB (network order) icon: a blue rounded square with white `< >`, drawn here so no image file is needed.
pub fn icon_argb(size: i32) -> Vec<u8> {
    let s = size as f32;
    let seg = |px: f32, py: f32, a: (f32, f32), b: (f32, f32)| {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let t = (((px - a.0) * dx + (py - a.1) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
        ((px - a.0 - t * dx).powi(2) + (py - a.1 - t * dy).powi(2)).sqrt()
    };
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let (cx, cy, r) = ((px - s / 2.0).abs() - (s / 2.0 - s * 0.22), (py - s / 2.0).abs() - (s / 2.0 - s * 0.22), s * 0.22);
            let d = (cx.max(0.0).powi(2) + cy.max(0.0).powi(2)).sqrt() + cx.max(cy).min(0.0) - r;
            let bg = (0.5 - d).clamp(0.0, 1.0);
            let w = s * 0.07;
            let l = [seg(px, py, (0.40 * s, 0.30 * s), (0.22 * s, 0.50 * s)), seg(px, py, (0.22 * s, 0.50 * s), (0.40 * s, 0.70 * s)), seg(px, py, (0.60 * s, 0.30 * s), (0.78 * s, 0.50 * s)), seg(px, py, (0.78 * s, 0.50 * s), (0.60 * s, 0.70 * s))];
            let fg = (w + 0.5 - l.iter().cloned().fold(f32::MAX, f32::min)).clamp(0.0, 1.0);
            let (r_, g_, b_) = (48.0 + (255.0 - 48.0) * fg, 102.0 + (255.0 - 102.0) * fg, 232.0 + (255.0 - 232.0) * fg);
            out.extend([(bg * 255.0) as u8, r_ as u8, g_ as u8, b_ as u8]);
        }
    }
    out
}

#[cfg(target_os = "linux")]
pub fn start_tray(url: String, own_window: bool) {
    use ksni::blocking::TrayMethods;
    struct Tray {
        url: String,
        own: bool,
    }
    impl ksni::Tray for Tray {
        fn id(&self) -> String {
            "kempt".into()
        }
        fn title(&self) -> String {
            "Kempt".into()
        }
        fn icon_name(&self) -> String {
            String::new()
        }
        fn icon_pixmap(&self) -> Vec<ksni::Icon> {
            vec![ksni::Icon { width: 32, height: 32, data: icon_argb(32) }]
        }
        fn activate(&mut self, _x: i32, _y: i32) {
            open_ui(&self.url, self.own);
        }
        fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
            use ksni::menu::*;
            let item = |label: &str, suffix: &'static str| -> ksni::MenuItem<Self> {
                StandardItem { label: label.into(), activate: Box::new(move |t: &mut Self| open_ui(&format!("{}{}", t.url, suffix), t.own)), ..Default::default() }.into()
            };
            vec![
                item("Open Kempt", ""),
                item("Checklist & reminders", "&view=notes"),
                item("World clock", "&view=clock"),
                MenuItem::Separator,
                StandardItem {
                    label: "Quit Kempt".into(),
                    activate: Box::new(|_| {
                        clear_instance();
                        std::process::exit(0);
                    }),
                    ..Default::default()
                }
                .into(),
            ]
        }
    }
    std::thread::spawn(move || {
        // no tray host on this desktop (or no session bus)? then there is simply no icon
        if let Ok(h) = (Tray { url, own: own_window }).spawn() {
            std::mem::forget(h);
            loop {
                std::thread::park();
            }
        }
    });
}

#[cfg(not(target_os = "linux"))]
pub fn start_tray(_url: String, _own_window: bool) {}

#[cfg(test)]
mod tests {
    #[test]
    fn repo_names() {
        use super::valid_repo;
        assert!(valid_repo("alice/kempt") && valid_repo("a-b/c.d_e"));
        assert!(!valid_repo("alice") && !valid_repo("a/b/c") && !valid_repo("a/b?x=1") && !valid_repo("/b") && !valid_repo("a b/c") && !valid_repo("../x\u{0}/y") && !valid_repo("../..") && !valid_repo("./."));
    }
    #[test]
    fn version_order() {
        use super::is_newer;
        assert!(is_newer("v0.4.14", "0.4.13"));
        assert!(is_newer("0.5.0", "0.4.13"));
        assert!(is_newer("v0.4.13", "0.4.9"));
        assert!(!is_newer("0.4.13", "0.4.13"));
        assert!(!is_newer("v0.4.9", "0.4.13"));
        assert!(is_newer("1.0", "0.9.9"));
        assert!(!is_newer("nonsense", "0.4.13"));
    }

    use super::*;

    #[test]
    fn wav_is_well_formed() {
        let w = alert_wav();
        assert_eq!(&w[0..4], b"RIFF");
        assert_eq!(&w[8..16], b"WAVEfmt ");
        let data_len = u32::from_le_bytes(w[40..44].try_into().unwrap()) as usize;
        assert_eq!(w.len(), 44 + data_len);
        assert_eq!(u32::from_le_bytes(w[4..8].try_into().unwrap()) as usize, 36 + data_len);
        assert!(w[44..].iter().any(|b| *b != 0), "not silent");
    }

    #[test]
    fn tray_icon_has_the_right_size_and_a_transparent_corner() {
        let d = icon_argb(32);
        assert_eq!(d.len(), 32 * 32 * 4);
        assert_eq!(d[0], 0, "corner is transparent");
        let mid = (16 * 32 + 3) * 4;
        assert_eq!(d[mid], 255, "body is opaque");
    }

    #[test]
    fn percent_encoding() {
        assert_eq!(pct("/tmp/a b&c.txt"), "/tmp/a%20b%26c.txt");
        assert_eq!(pct("C:\\x"), "C%3A%5Cx");
    }
}

// ---------------------------------------------------------------- opt-in update check
/// "owner/repo" of the project on GitHub. Set when building (`KEMPT_REPO=owner/kempt cargo build`; the release workflow does it from
/// the repository it runs in) or at run time with the same variable. `OWNER/kempt` means "not configured".
fn repo() -> String {
    std::env::var("KEMPT_REPO").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| option_env!("KEMPT_REPO").unwrap_or("OWNER/kempt").to_string())
}

/// `owner/name` with only the characters GitHub allows (it goes into a URL).
pub fn valid_repo(r: &str) -> bool {
    let mut it = r.split('/');
    let ok = |p: &str| !p.is_empty() && p != "." && p != ".." && p.len() <= 100 && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.');
    matches!((it.next(), it.next(), it.next()), (Some(a), Some(b), None) if ok(a) && ok(b))
}

/// Numeric parts of a version such as "v0.4.13" -> [0, 4, 13].
fn ver_parts(v: &str) -> Vec<u64> {
    v.trim().trim_start_matches(['v', 'V']).split(['.', '-', '+']).map_while(|p| p.parse::<u64>().ok()).collect()
}

pub fn is_newer(latest: &str, current: &str) -> bool {
    let (a, b) = (ver_parts(latest), ver_parts(current));
    let n = a.len().max(b.len());
    for i in 0..n {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x > y;
        }
    }
    false
}

/// Ask GitHub for the latest release (one HTTPS request, made only when the user turned the check on or pressed "Check now").
/// Uses the system `curl`, so the program itself carries no network or TLS code. Never downloads or installs anything.
pub fn update_check(chosen: &str) -> Result<serde_json::Value, String> {
    let chosen = chosen.trim();
    if !chosen.is_empty() && !valid_repo(chosen) {
        return Err("The repository must look like owner/name.".into());
    }
    let url = std::env::var("KEMPT_UPDATE_URL").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| {
        let r = if chosen.is_empty() { repo() } else { chosen.to_string() };
        format!("https://api.github.com/repos/{r}/releases/latest")
    });
    if url.contains("OWNER/kempt") {
        return Err("The update address is not set up in this build (the project's GitHub repository is unknown).".into());
    }
    let out = Command::new("curl")
        .args(["-fsSL", "--max-time", "8", "-H", "Accept: application/vnd.github+json", "-A", "kempt-update-check", &url])
        .stdin(Stdio::null())
        .output()
        .map_err(|_| "curl was not found on this computer, so Kempt cannot check.".to_string())?;
    if !out.status.success() {
        return Err("Could not reach the release page (offline, or no release has been published yet).".into());
    }
    let j: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|_| "The release page gave an unexpected answer.".to_string())?;
    let tag = j["tag_name"].as_str().ok_or("The release page has no version.")?.to_string();
    let page = j["html_url"].as_str().unwrap_or("").to_string();
    let cur = env!("CARGO_PKG_VERSION");
    let page = if page.starts_with("https://") { page } else { String::new() };
    Ok(serde_json::json!({"current": cur, "latest": tag.trim_start_matches(['v', 'V']), "newer": is_newer(&tag, cur), "url": page}))
}
