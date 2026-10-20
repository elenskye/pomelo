//! 本地预览: the site's own dev server (`npm run dev -- --port N`, Astro) in its own process group.
//! Running = something answers on 127.0.0.1:N. One started elsewhere (a terminal) is used as it is and
//! never stopped; ours stops on 停止 and when Pomelo quits.

use super::tools::{self, Log};
use serde::Serialize;
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::process::Child;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const START_WAIT: Duration = Duration::from_secs(120);

#[derive(Default)]
struct Inner {
    child: Option<Child>,
    starting: bool,
    error: Option<String>,
    log: Log,
}

#[derive(Default)]
pub struct Server {
    inner: Mutex<Inner>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ServerView {
    /// stopped | starting | running
    pub state: &'static str,
    /// Started by Pomelo (so 停止 is offered).
    pub ours: bool,
    pub url: String,
    pub error: Option<String>,
}

pub fn port_open(port: u16) -> bool {
    TcpStream::connect_timeout(&SocketAddr::from(([127, 0, 0, 1], port)), Duration::from_millis(250)).is_ok()
}

pub fn url(port: u16) -> String {
    format!("http://127.0.0.1:{port}/")
}

fn log_tail(log: &Log) -> String {
    let lines: Vec<String> = log.tail(8).into_iter().filter(|l| !l.trim().is_empty()).collect();
    lines.join("\n")
}

impl Server {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Forgets a child that has exited (a start that died leaves its last lines as the error).
    fn reap(g: &mut Inner) {
        if let Some(c) = g.child.as_mut() {
            if let Ok(Some(status)) = c.try_wait() {
                g.child = None;
                if g.starting || !status.success() {
                    let tail = log_tail(&g.log);
                    g.error = Some(if tail.is_empty() { "本地服务停了".into() } else { format!("本地服务停了：\n{tail}") });
                }
                g.starting = false;
            }
        }
    }

    pub fn view(&self, port: u16) -> ServerView {
        let mut g = self.lock();
        Self::reap(&mut g);
        let open = port_open(port);
        let state = if open {
            "running"
        } else if g.starting {
            "starting"
        } else {
            "stopped"
        };
        ServerView { state, ours: g.child.is_some(), url: url(port), error: if open { None } else { g.error.clone() } }
    }

    /// Starts `npm run dev` unless something already answers on the port; returns at once.
    pub fn start(&self, folder: &Path, node_bin: &Path, port: u16) -> Result<(), String> {
        let mut g = self.lock();
        Self::reap(&mut g);
        if port_open(port) || g.child.is_some() {
            return Ok(());
        }
        if !folder.join("package.json").is_file() {
            return Err("网站文件夹里没有 package.json".into());
        }
        if !folder.join("node_modules").is_dir() {
            return Err("网站还没装依赖（node_modules）：先在网站文件夹里跑一次 npm ci".into());
        }
        g.log = Log::default();
        let port_s = port.to_string();
        let child = tools::spawn_logged(&node_bin.join("npm"), folder, &["run", "dev", "--", "--port", &port_s], Some(node_bin), &g.log)?;
        (g.child, g.starting, g.error) = (Some(child), true, None);
        Ok(())
    }

    /// Waits (up to `limit`) until the port answers or our start dies.
    pub fn wait_ready(&self, port: u16, limit: Duration) -> Result<ServerView, String> {
        let begun = Instant::now();
        loop {
            {
                let mut g = self.lock();
                Self::reap(&mut g);
                if port_open(port) {
                    g.starting = false;
                    g.error = None;
                    drop(g);
                    return Ok(self.view(port));
                }
                if g.child.is_none() {
                    g.starting = false;
                    return Err(g.error.clone().unwrap_or_else(|| "本地服务没有启动".into()));
                }
                if begun.elapsed() > limit {
                    g.starting = false;
                    let pid = g.child.as_ref().map(|c| c.id());
                    g.child = None;
                    drop(g);
                    if let Some(p) = pid {
                        tools::kill_group(p);
                    }
                    return Err(format!("本地服务 {} 秒还没启动好，已停止", limit.as_secs()));
                }
            }
            std::thread::sleep(Duration::from_millis(300));
        }
    }

    /// Stops ours (the whole group); one started elsewhere is left alone.
    pub fn stop(&self) {
        let mut g = self.lock();
        if let Some(mut c) = g.child.take() {
            tools::kill_group(c.id());
            std::thread::spawn(move || {
                let _ = c.wait();
            });
        }
        (g.starting, g.error) = (false, None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::temp_dir;

    /// A fake npm: `run dev -- --port N` listens on N (macOS's Perl, with a backlog so quick probes always connect).
    fn fake_npm(bin: &Path, body: &str) {
        std::fs::create_dir_all(bin).unwrap();
        std::fs::write(bin.join("npm"), format!("#!/bin/sh\n{body}\n")).unwrap();
        std::process::Command::new("/bin/chmod").arg("+x").arg(bin.join("npm")).status().unwrap();
    }

    fn free_port() -> u16 {
        std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
    }

    #[test]
    fn starts_waits_and_stops_its_own() {
        let dir = temp_dir("site-server");
        let (site, bin) = (dir.join("site"), dir.join("bin"));
        std::fs::create_dir_all(site.join("node_modules")).unwrap();
        std::fs::write(site.join("package.json"), "{}").unwrap();
        // listens after a moment, like astro dev
        fake_npm(&bin, "sleep 0.5; exec /usr/bin/perl -MIO::Socket::INET -e '$s = IO::Socket::INET->new(LocalAddr => \"127.0.0.1\", LocalPort => $ARGV[0], Listen => 64, ReuseAddr => 1) or die; while ($c = $s->accept) { close $c }' \"$5\"");
        let port = free_port();
        let s = Server::default();
        assert_eq!(s.view(port).state, "stopped");
        s.start(&site, &bin, port).unwrap();
        assert_eq!(s.view(port).state, "starting");
        let v = s.wait_ready(port, Duration::from_secs(10)).unwrap();
        assert_eq!((v.state, v.ours, v.url.as_str()), ("running", true, url(port).as_str()));
        s.stop();
        let gone = (0..50).any(|_| {
            std::thread::sleep(Duration::from_millis(100));
            !port_open(port)
        });
        assert!(gone && s.view(port).state == "stopped");
    }

    #[test]
    fn a_failed_start_says_why_and_a_foreign_one_is_left_alone() {
        let dir = temp_dir("site-server-fail");
        let (site, bin) = (dir.join("site"), dir.join("bin"));
        std::fs::create_dir_all(site.join("node_modules")).unwrap();
        std::fs::write(site.join("package.json"), "{}").unwrap();
        fake_npm(&bin, "echo 'Error: Cannot find module astro' >&2; exit 1");
        let port = free_port();
        let s = Server::default();
        s.start(&site, &bin, port).unwrap();
        assert!(s.wait_ready(port, Duration::from_secs(10)).unwrap_err().contains("Cannot find module astro"));
        assert!(s.view(port).error.unwrap().contains("astro"));
        // something else on the port: used, not ours, never stopped
        let foreign = std::net::TcpListener::bind(("127.0.0.1", port)).unwrap();
        s.start(&site, &bin, port).unwrap();
        let v = s.view(port);
        assert_eq!((v.state, v.ours, v.error), ("running", false, None));
        s.stop();
        assert!(port_open(port));
        drop(foreign);
        std::fs::remove_dir_all(site.join("node_modules")).unwrap();
        assert!(s.start(&site, &bin, port).unwrap_err().contains("npm ci"));
    }
}
