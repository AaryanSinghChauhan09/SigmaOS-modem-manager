//! SigmaOS-modem-manager — modem manager CLI.
//!
//! Usage: mm2 <state.json> <command> [args]
//! Commands: modem-add, modems, profile-add, profiles, probe, connect,
//!           disconnect, at, sms-send, sms-store, sms-list, stats

use sigmaos_modem_manager::{
    ApnProfile, AtChannel, AuthType, IpType, Modem, ModemManager, ModemState, SmsStatus, TtyChannel,
};
use std::path::PathBuf;
use std::process::exit;

fn usage() -> ! {
    eprintln!("usage: mm2 <state.json> <modem-add|modems|profile-add|profiles|probe|connect|disconnect|at|sms-send|sms-store|sms-list|stats>");
    exit(2);
}

fn load(path: &PathBuf) -> Result<ModemManager, String> {
    ModemManager::load(path).map_err(|e| e.to_string())
}

fn save(m: &ModemManager, path: &PathBuf) -> Result<(), String> {
    m.save(path).map_err(|e| e.to_string())
}

fn auth(s: &str) -> Result<AuthType, String> {
    match s {
        "none" => Ok(AuthType::None),
        "pap" => Ok(AuthType::Pap),
        "chap" => Ok(AuthType::Chap),
        _ => Err("auth none|pap|chap".into()),
    }
}
fn iptype(s: &str) -> Result<IpType, String> {
    match s {
        "v4" | "ip" => Ok(IpType::V4),
        "v6" | "ipv6" => Ok(IpType::V6),
        "v4v6" | "ipv4v6" => Ok(IpType::V4v6),
        _ => Err("ip v4|v6|v4v6".into()),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        usage();
    }
    let state = PathBuf::from(&args[0]);
    let cmd = args[1].as_str();
    let rest = &args[2..];
    let mut m = match load(&state) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("mm2: {}", e);
            exit(1);
        }
    };

    let result: Result<(), String> = (|| {
        match cmd {
            "modem-add" => {
                if rest.len() < 3 {
                    return Err("modem-add <id> <name> <port> [caps-csv]".into());
                }
                let caps = rest
                    .get(3)
                    .map(|s| s.split(',').map(|x| x.trim().to_string()).collect())
                    .unwrap_or_else(|| vec!["gsm".into()]);
                m.add_modem(Modem {
                    id: rest[0].clone(),
                    name: rest[1].clone(),
                    port: PathBuf::from(&rest[2]),
                    state: ModemState::Disconnected,
                    signal: None,
                    operator: None,
                    apn: None,
                    caps,
                })
                .map_err(|e| e.to_string())?;
                println!("added {}", rest[0]);
            }
            "modems" => {
                for md in m.modems.values() {
                    println!(
                        "{:<12} {:<10} {:<12} port={} signal={} op={}",
                        md.id,
                        md.name,
                        md.state.name(),
                        md.port.display(),
                        md.signal
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| "-".into()),
                        md.operator.clone().unwrap_or_else(|| "-".into())
                    );
                }
                println!("{} modems", m.modems.len());
            }
            "profile-add" => {
                if rest.len() < 2 {
                    return Err("profile-add <name> <apn> [user] [pass] [auth] [iptype]".into());
                }
                let user = rest.get(2).cloned();
                let pass = rest.get(3).cloned();
                let a = rest
                    .get(4)
                    .map(|s| auth(s))
                    .transpose()?
                    .unwrap_or(AuthType::None);
                let ip = rest
                    .get(5)
                    .map(|s| iptype(s))
                    .transpose()?
                    .unwrap_or(IpType::V4);
                m.add_profile(ApnProfile {
                    name: rest[0].clone(),
                    apn: rest[1].clone(),
                    username: user,
                    password: pass,
                    auth: a,
                    ip_type: ip,
                })
                .map_err(|e| e.to_string())?;
                println!("added profile {}", rest[0]);
            }
            "profiles" => {
                for p in m.profiles.values() {
                    println!(
                        "{:<16} apn={} auth={} ip={}",
                        p.name,
                        p.apn,
                        p.auth.name(),
                        p.ip_type.name()
                    );
                }
                println!("{} profiles", m.profiles.len());
            }
            "probe" => {
                if rest.is_empty() {
                    return Err("probe <modem>".into());
                }
                let port = m.get_modem(&rest[0]).ok_or("modem not found")?.port.clone();
                let mut ch = TtyChannel::open(&port).map_err(|e| e.to_string())?;
                let md = m.probe(&rest[0], &mut ch).map_err(|e| e.to_string())?;
                println!(
                    "{} state={} signal={} op={}",
                    md.id,
                    md.state.name(),
                    md.signal
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| "-".into()),
                    md.operator.unwrap_or_else(|| "-".into())
                );
            }
            "connect" => {
                if rest.len() < 2 {
                    return Err("connect <modem> <profile>".into());
                }
                let port = m.get_modem(&rest[0]).ok_or("modem not found")?.port.clone();
                let mut ch = TtyChannel::open(&port).map_err(|e| e.to_string())?;
                m.connect(&rest[0], &rest[1], &mut ch)
                    .map_err(|e| e.to_string())?;
                println!("connected {} via {}", rest[0], rest[1]);
            }
            "disconnect" => {
                if rest.is_empty() {
                    return Err("disconnect <modem>".into());
                }
                let port = m.get_modem(&rest[0]).ok_or("modem not found")?.port.clone();
                let mut ch = TtyChannel::open(&port).map_err(|e| e.to_string())?;
                m.disconnect(&rest[0], &mut ch).map_err(|e| e.to_string())?;
                println!("disconnected");
            }
            "at" => {
                if rest.len() < 2 {
                    return Err("at <modem> <command>".into());
                }
                let port = m.get_modem(&rest[0]).ok_or("modem not found")?.port.clone();
                let mut ch = TtyChannel::open(&port).map_err(|e| e.to_string())?;
                let resp = ch
                    .transact(&rest[1..].join(" "))
                    .map_err(|e| e.to_string())?;
                print!("{}", resp);
            }
            "sms-send" => {
                if rest.len() < 3 {
                    return Err("sms-send <modem> <number> <text...>".into());
                }
                let port = m.get_modem(&rest[0]).ok_or("modem not found")?.port.clone();
                let mut ch = TtyChannel::open(&port).map_err(|e| e.to_string())?;
                let id = m
                    .send_sms(&rest[0], &rest[1], &rest[2..].join(" "), &mut ch)
                    .map_err(|e| e.to_string())?;
                println!("{}", id);
            }
            "sms-store" => {
                if rest.len() < 3 {
                    return Err("sms-store <modem> <number> <text...>".into());
                }
                let id = m
                    .store_sms(&rest[0], &rest[1], &rest[2..].join(" "), SmsStatus::Stored)
                    .map_err(|e| e.to_string())?;
                println!("{}", id);
            }
            "sms-list" => {
                for s in m.sms.values() {
                    println!(
                        "{:<10} {:<8} {} -> {} {}",
                        s.id,
                        s.modem,
                        s.number,
                        s.status.name(),
                        s.text
                    );
                }
                println!("{} sms", m.sms.len());
            }
            "stats" => {
                let s = m.stats();
                println!(
                    "{} modems, {} connected, {} profiles, {} sms ({} unread)",
                    s.modems, s.connected, s.profiles, s.sms, s.unread_sms
                );
            }
            _ => usage(),
        }
        Ok(())
    })();
    if let Err(e) = result {
        eprintln!("mm2: {}", e);
        exit(1);
    }
    if let Err(e) = save(&m, &state) {
        eprintln!("mm2: save: {}", e);
    }
}
