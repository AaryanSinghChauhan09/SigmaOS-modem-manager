// SPDX-License-Identifier: Apache-2.0
//!
//! SigmaOS-modem-manager — modem manager.
//!
//! Manages modems, APN profiles, AT-command sessions, connection state,
//! signal/operator queries, and SMS records through an injectable
//! channel. Includes a TTY channel for real serial ports.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;

/// Modem connection state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModemState {
    Disconnected,
    Searching,
    Registered,
    Connected,
    Failed(String),
}
impl ModemState {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Disconnected => "disconnected",
            Self::Searching => "searching",
            Self::Registered => "registered",
            Self::Connected => "connected",
            Self::Failed(_) => "failed",
        }
    }
}

/// APN authentication type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthType {
    None,
    Pap,
    Chap,
}
impl AuthType {
    pub fn name(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Pap => "pap",
            Self::Chap => "chap",
        }
    }
}

/// IP type for PDP context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IpType {
    V4,
    V6,
    V4v6,
}
impl IpType {
    pub fn name(&self) -> &'static str {
        match self {
            Self::V4 => "IP",
            Self::V6 => "IPV6",
            Self::V4v6 => "IPV4V6",
        }
    }
}

/// APN profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApnProfile {
    pub name: String,
    pub apn: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub auth: AuthType,
    pub ip_type: IpType,
}

/// Modem record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Modem {
    pub id: String,
    pub name: String,
    pub port: PathBuf,
    pub state: ModemState,
    pub signal: Option<u8>, // 0-31 rssi
    pub operator: Option<String>,
    pub apn: Option<String>,
    pub caps: Vec<String>,
}

/// SMS record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sms {
    pub id: String,
    pub modem: String,
    pub number: String,
    pub text: String,
    pub timestamp: u64,
    pub read: bool,
    pub status: SmsStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SmsStatus {
    Stored,
    Sent,
    Failed,
}
impl SmsStatus {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Stored => "stored",
            Self::Sent => "sent",
            Self::Failed => "failed",
        }
    }
}

/// AT command channel.
pub trait AtChannel {
    fn transact(&mut self, command: &str) -> Result<String, String>;
}

/// Scripted channel for tests/demos.
#[derive(Default)]
pub struct FakeChannel {
    pub script: BTreeMap<String, String>,
    pub sent: Vec<String>,
}
impl FakeChannel {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn respond(mut self, command: &str, response: &str) -> Self {
        self.script
            .insert(command.to_string(), response.to_string());
        self
    }
}
impl AtChannel for FakeChannel {
    fn transact(&mut self, command: &str) -> Result<String, String> {
        self.sent.push(command.to_string());
        self.script
            .get(command)
            .cloned()
            .ok_or_else(|| format!("no scripted response for {}", command))
    }
}

/// TTY channel for real serial ports.
pub struct TtyChannel {
    writer: std::fs::File,
    rx: Receiver<String>,
    timeout: Duration,
}
impl TtyChannel {
    pub fn open(port: &Path) -> Result<Self, String> {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(port)
            .map_err(|e| format!("open {}: {}", port.display(), e))?;
        let reader = file.try_clone().map_err(|e| e.to_string())?;
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let mut lines = BufReader::new(reader).lines();
            while let Some(Ok(line)) = lines.next() {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            writer: file,
            rx,
            timeout: Duration::from_millis(2000),
        })
    }
}
impl AtChannel for TtyChannel {
    fn transact(&mut self, command: &str) -> Result<String, String> {
        writeln!(self.writer, "{}\r", command).map_err(|e| e.to_string())?;
        self.writer.flush().map_err(|e| e.to_string())?;
        let deadline = std::time::Instant::now() + self.timeout;
        let mut out = String::new();
        while std::time::Instant::now() < deadline {
            match self.rx.recv_timeout(Duration::from_millis(100)) {
                Ok(line) => {
                    out.push_str(&line);
                    out.push('\n');
                    let up = out.to_uppercase();
                    if up.contains("OK") || up.contains("ERROR") || up.contains("CONNECT") {
                        break;
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        if out.is_empty() {
            return Err("timeout waiting for modem".into());
        }
        Ok(out)
    }
}

/// Errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MmError {
    DuplicateModem(String),
    DuplicateProfile(String),
    ModemNotFound(String),
    ProfileNotFound(String),
    SmsNotFound(String),
    At(String),
    BadResponse(String),
    Io(String),
    Json(String),
}
impl fmt::Display for MmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateModem(id) => write!(f, "duplicate modem {}", id),
            Self::DuplicateProfile(n) => write!(f, "duplicate profile {}", n),
            Self::ModemNotFound(id) => write!(f, "modem {} not found", id),
            Self::ProfileNotFound(n) => write!(f, "profile {} not found", n),
            Self::SmsNotFound(id) => write!(f, "sms {} not found", id),
            Self::At(m) => write!(f, "at: {}", m),
            Self::BadResponse(m) => write!(f, "bad response: {}", m),
            Self::Io(m) => write!(f, "io: {}", m),
            Self::Json(m) => write!(f, "json: {}", m),
        }
    }
}
impl std::error::Error for MmError {}

fn has_ok(resp: &str) -> bool {
    resp.to_uppercase().contains("OK")
}
fn has_error(resp: &str) -> bool {
    resp.to_uppercase().contains("ERROR")
}
fn has_connect(resp: &str) -> bool {
    resp.to_uppercase().contains("CONNECT")
}

/// Modem manager.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModemManager {
    pub modems: BTreeMap<String, Modem>,
    pub profiles: BTreeMap<String, ApnProfile>,
    pub sms: BTreeMap<String, Sms>,
    next_sms: u64,
}

impl ModemManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_modem(&mut self, modem: Modem) -> Result<(), MmError> {
        if self.modems.contains_key(&modem.id) {
            return Err(MmError::DuplicateModem(modem.id));
        }
        self.modems.insert(modem.id.clone(), modem);
        Ok(())
    }

    pub fn remove_modem(&mut self, id: &str) -> Result<Modem, MmError> {
        self.modems
            .remove(id)
            .ok_or_else(|| MmError::ModemNotFound(id.into()))
    }

    pub fn get_modem(&self, id: &str) -> Option<&Modem> {
        self.modems.get(id)
    }
    pub fn get_modem_mut(&mut self, id: &str) -> Option<&mut Modem> {
        self.modems.get_mut(id)
    }

    pub fn add_profile(&mut self, profile: ApnProfile) -> Result<(), MmError> {
        if self.profiles.contains_key(&profile.name) {
            return Err(MmError::DuplicateProfile(profile.name));
        }
        self.profiles.insert(profile.name.clone(), profile);
        Ok(())
    }

    pub fn remove_profile(&mut self, name: &str) -> Result<ApnProfile, MmError> {
        self.profiles
            .remove(name)
            .ok_or_else(|| MmError::ProfileNotFound(name.into()))
    }

    pub fn get_profile(&self, name: &str) -> Option<&ApnProfile> {
        self.profiles.get(name)
    }

    fn expect_ok(&self, channel: &mut dyn AtChannel, cmd: &str) -> Result<String, MmError> {
        let resp = channel.transact(cmd).map_err(MmError::At)?;
        if has_error(&resp) {
            return Err(MmError::BadResponse(format!("{} -> {}", cmd, resp.trim())));
        }
        if !has_ok(&resp) && !has_connect(&resp) {
            return Err(MmError::BadResponse(format!("{} missing OK", cmd)));
        }
        Ok(resp)
    }

    /// Probe modem: AT, CGSN, CSQ, COPS.
    pub fn probe(&mut self, id: &str, channel: &mut dyn AtChannel) -> Result<Modem, MmError> {
        if !self.modems.contains_key(id) {
            return Err(MmError::ModemNotFound(id.into()));
        }
        self.expect_ok(channel, "AT")?;
        self.expect_ok(channel, "ATE0")?;
        let mut signal = None;
        let mut operator = None;
        if let Ok(resp) = channel.transact("AT+CSQ") {
            signal = parse_csq(&resp);
        }
        if let Ok(resp) = channel.transact("AT+COPS?") {
            operator = parse_cops(&resp);
        }
        let modem = self.modems.get_mut(id).unwrap();
        if signal.is_some() {
            modem.signal = signal;
        }
        if operator.is_some() {
            modem.operator = operator;
        }
        modem.state = ModemState::Registered;
        Ok(modem.clone())
    }

    /// Connect using an APN profile.
    pub fn connect(
        &mut self,
        id: &str,
        profile_name: &str,
        channel: &mut dyn AtChannel,
    ) -> Result<(), MmError> {
        let profile = self
            .profiles
            .get(profile_name)
            .ok_or_else(|| MmError::ProfileNotFound(profile_name.into()))?
            .clone();
        if !self.modems.contains_key(id) {
            return Err(MmError::ModemNotFound(id.into()));
        }
        {
            let modem = self.modems.get_mut(id).unwrap();
            modem.state = ModemState::Searching;
            modem.apn = Some(profile.apn.clone());
        }
        self.expect_ok(channel, "AT")?;
        self.expect_ok(channel, "AT+CGATT=1")?;
        let ctx = format!(
            "AT+CGDCONT=1,\"{}\",\"{}\",\"{}\",0,0,,,,,,,",
            profile.ip_type.name(),
            profile.apn,
            profile.username.clone().unwrap_or_default()
        );
        self.expect_ok(channel, &ctx)?;
        let resp = channel.transact("ATD*99***1#").map_err(MmError::At)?;
        let modem = self.modems.get_mut(id).unwrap();
        if has_connect(&resp) || has_ok(&resp) {
            modem.state = ModemState::Connected;
            Ok(())
        } else {
            modem.state = ModemState::Failed(resp.trim().to_string());
            Err(MmError::BadResponse(resp.trim().to_string()))
        }
    }

    pub fn disconnect(&mut self, id: &str, channel: &mut dyn AtChannel) -> Result<(), MmError> {
        if !self.modems.contains_key(id) {
            return Err(MmError::ModemNotFound(id.into()));
        }
        let _ = channel.transact("ATH");
        self.modems.get_mut(id).unwrap().state = ModemState::Disconnected;
        Ok(())
    }

    /// Send SMS and store record.
    pub fn send_sms(
        &mut self,
        id: &str,
        number: &str,
        text: &str,
        channel: &mut dyn AtChannel,
    ) -> Result<String, MmError> {
        let modem_id = id.to_string();
        if !self.modems.contains_key(&modem_id) {
            return Err(MmError::ModemNotFound(modem_id));
        }
        self.expect_ok(channel, "AT+CMGF=1")?;
        channel
            .transact(&format!("AT+CMGS=\"{}\"", number))
            .map_err(MmError::At)?;
        // channel writes text + Ctrl-Z; for scripted channels send combined command marker
        let resp = channel
            .transact(&format!("{}\u{1a}", text))
            .map_err(MmError::At)?;
        let status = if has_ok(&resp) {
            SmsStatus::Sent
        } else {
            SmsStatus::Failed
        };
        let sms_id = format!("sms-{}", self.next_sms);
        self.next_sms += 1;
        self.sms.insert(
            sms_id.clone(),
            Sms {
                id: sms_id.clone(),
                modem: modem_id,
                number: number.into(),
                text: text.into(),
                timestamp: now_ts(),
                read: true,
                status,
            },
        );
        if status == SmsStatus::Failed {
            return Err(MmError::BadResponse(resp.trim().to_string()));
        }
        Ok(sms_id)
    }

    /// Store an incoming/stored SMS manually.
    pub fn store_sms(
        &mut self,
        modem: &str,
        number: &str,
        text: &str,
        status: SmsStatus,
    ) -> Result<String, MmError> {
        if !self.modems.contains_key(modem) {
            return Err(MmError::ModemNotFound(modem.into()));
        }
        let id = format!("sms-{}", self.next_sms);
        self.next_sms += 1;
        self.sms.insert(
            id.clone(),
            Sms {
                id: id.clone(),
                modem: modem.into(),
                number: number.into(),
                text: text.into(),
                timestamp: now_ts(),
                read: false,
                status,
            },
        );
        Ok(id)
    }

    pub fn mark_sms_read(&mut self, id: &str) -> Result<(), MmError> {
        let sms = self
            .sms
            .get_mut(id)
            .ok_or_else(|| MmError::SmsNotFound(id.into()))?;
        sms.read = true;
        Ok(())
    }

    pub fn stats(&self) -> MmStats {
        MmStats {
            modems: self.modems.len(),
            connected: self
                .modems
                .values()
                .filter(|m| m.state == ModemState::Connected)
                .count(),
            profiles: self.profiles.len(),
            sms: self.sms.len(),
            unread_sms: self.sms.values().filter(|s| !s.read).count(),
        }
    }

    pub fn to_json(&self) -> Result<String, MmError> {
        serde_json::to_string(self).map_err(|e| MmError::Json(e.to_string()))
    }
    pub fn from_json(text: &str) -> Result<Self, MmError> {
        serde_json::from_str(text).map_err(|e| MmError::Json(e.to_string()))
    }
    pub fn load(path: &Path) -> Result<Self, MmError> {
        match std::fs::read_to_string(path) {
            Ok(t) => Self::from_json(&t),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::new()),
            Err(e) => Err(MmError::Io(e.to_string())),
        }
    }
    pub fn save(&self, path: &Path) -> Result<(), MmError> {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d).map_err(|e| MmError::Io(e.to_string()))?;
        }
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, self.to_json()?).map_err(|e| MmError::Io(e.to_string()))?;
        std::fs::rename(&tmp, path).map_err(|e| MmError::Io(e.to_string()))
    }
}

#[derive(Debug, Clone)]
pub struct MmStats {
    pub modems: usize,
    pub connected: usize,
    pub profiles: usize,
    pub sms: usize,
    pub unread_sms: usize,
}

fn now_ts() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn parse_csq(resp: &str) -> Option<u8> {
    for line in resp.lines() {
        if let Some(rest) = line.trim().strip_prefix("+CSQ:") {
            let rssi = rest.split(',').next()?.trim().parse::<u8>().ok()?;
            // rssi 99 = unknown; 0-31 valid, 32-99 map to 0
            return Some(if rssi == 99 { 0 } else { rssi.min(31) });
        }
    }
    None
}

fn parse_cops(resp: &str) -> Option<String> {
    for line in resp.lines() {
        let line = line.trim();
        if line.starts_with("+COPS:") {
            // +COPS: 0,0,"Operator",13
            if let Some(start) = line.find('"') {
                if let Some(end) = line[start + 1..].find('"') {
                    return Some(line[start + 1..start + 1 + end].to_string());
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn modem(id: &str) -> Modem {
        Modem {
            id: id.into(),
            name: id.into(),
            port: PathBuf::from("/dev/ttyUSB0"),
            state: ModemState::Disconnected,
            signal: None,
            operator: None,
            apn: None,
            caps: vec!["gsm".into()],
        }
    }

    fn profile(name: &str, apn: &str) -> ApnProfile {
        ApnProfile {
            name: name.into(),
            apn: apn.into(),
            username: None,
            password: None,
            auth: AuthType::None,
            ip_type: IpType::V4,
        }
    }

    fn ok_channel() -> FakeChannel {
        FakeChannel::new()
            .respond("AT", "AT\r\nOK")
            .respond("ATE0", "ATE0\r\nOK")
            .respond("AT+CGATT=1", "AT+CGATT=1\r\nOK")
            .respond(
                "AT+CGDCONT=1,\"IP\",\"internet\",\"\",0,0,,,,,,,",
                "AT+CGDCONT=1\r\nOK",
            )
            .respond("ATD*99***1#", "CONNECT 115200")
            .respond("ATH", "ATH\r\nOK")
            .respond("AT+CSQ", "AT+CSQ\r\n+CSQ: 21,99\r\nOK")
            .respond("AT+COPS?", "AT+COPS?\r\n+COPS: 0,0,\"TestOp\",13\r\nOK")
            .respond("AT+CMGF=1", "AT+CMGF=1\r\nOK")
            .respond("AT+CMGS=\"+1555\"", "AT+CMGS=\"+1555\"\r\n>")
            .respond("hello\u{1a}", "hello\r\n+CMGS: 0\r\nOK")
    }

    #[test]
    fn add_modem_profile() {
        let mut m = ModemManager::new();
        m.add_modem(modem("m1")).unwrap();
        m.add_profile(profile("internet", "internet.apn")).unwrap();
        assert_eq!(m.modems.len(), 1);
        assert_eq!(m.profiles.len(), 1);
        assert!(m.add_modem(modem("m1")).is_err());
        assert!(m.add_profile(profile("internet", "x")).is_err());
    }

    #[test]
    fn probe_updates_state() {
        let mut m = ModemManager::new();
        m.add_modem(modem("m1")).unwrap();
        let mut ch = ok_channel();
        let md = m.probe("m1", &mut ch).unwrap();
        assert_eq!(md.state, ModemState::Registered);
        assert_eq!(md.signal, Some(21));
        assert_eq!(md.operator.as_deref(), Some("TestOp"));
    }

    #[test]
    fn connect_disconnect() {
        let mut m = ModemManager::new();
        m.add_modem(modem("m1")).unwrap();
        m.add_profile(profile("internet", "internet")).unwrap();
        let mut ch = ok_channel();
        m.connect("m1", "internet", &mut ch).unwrap();
        assert_eq!(m.get_modem("m1").unwrap().state, ModemState::Connected);
        assert_eq!(m.get_modem("m1").unwrap().apn.as_deref(), Some("internet"));
        m.disconnect("m1", &mut ch).unwrap();
        assert_eq!(m.get_modem("m1").unwrap().state, ModemState::Disconnected);
    }

    #[test]
    fn connect_failure_state() {
        let mut m = ModemManager::new();
        m.add_modem(modem("m1")).unwrap();
        m.add_profile(profile("bad", "bad.apn")).unwrap();
        let mut ch = FakeChannel::new()
            .respond("AT", "OK")
            .respond("AT+CGATT=1", "OK")
            .respond("AT+CGDCONT=1,\"IP\",\"bad.apn\",\"\",0,0,,,,,,,", "OK")
            .respond("ATD*99***1#", "ERROR");
        assert!(m.connect("m1", "bad", &mut ch).is_err());
        assert!(matches!(
            m.get_modem("m1").unwrap().state,
            ModemState::Failed(_)
        ));
    }

    #[test]
    fn send_sms_stores() {
        let mut m = ModemManager::new();
        m.add_modem(modem("m1")).unwrap();
        let mut ch = ok_channel();
        let id = m.send_sms("m1", "+1555", "hello", &mut ch).unwrap();
        let sms = m.sms.get(&id).unwrap();
        assert_eq!(sms.status, SmsStatus::Sent);
        assert_eq!(sms.number, "+1555");
    }

    #[test]
    fn store_and_read_sms() {
        let mut m = ModemManager::new();
        m.add_modem(modem("m1")).unwrap();
        let id = m
            .store_sms("m1", "+1666", "incoming", SmsStatus::Stored)
            .unwrap();
        assert!(!m.sms.get(&id).unwrap().read);
        m.mark_sms_read(&id).unwrap();
        assert!(m.sms.get(&id).unwrap().read);
        assert_eq!(m.stats().unread_sms, 0);
    }

    #[test]
    fn csq_parse() {
        assert_eq!(parse_csq("+CSQ: 21,99"), Some(21));
        assert_eq!(parse_csq("+CSQ: 99,99"), Some(0));
        assert_eq!(parse_csq("no"), None);
    }

    #[test]
    fn cops_parse() {
        assert_eq!(
            parse_cops("+COPS: 0,0,\"Vodafone\",13"),
            Some("Vodafone".into())
        );
        assert_eq!(parse_cops("+COPS: 0"), None);
    }

    #[test]
    fn persistence() {
        let mut m = ModemManager::new();
        m.add_modem(modem("m1")).unwrap();
        m.add_profile(profile("internet", "apn")).unwrap();
        m.store_sms("m1", "+1", "hi", SmsStatus::Stored).unwrap();
        let j = m.to_json().unwrap();
        let m2 = ModemManager::from_json(&j).unwrap();
        assert_eq!(m2.modems.len(), 1);
        assert_eq!(m2.profiles.len(), 1);
        assert_eq!(m2.sms.len(), 1);
    }

    #[test]
    fn stats() {
        let mut m = ModemManager::new();
        m.add_modem(modem("m1")).unwrap();
        m.add_profile(profile("p", "apn")).unwrap();
        m.store_sms("m1", "+1", "x", SmsStatus::Stored).unwrap();
        let s = m.stats();
        assert_eq!(s.modems, 1);
        assert_eq!(s.profiles, 1);
        assert_eq!(s.sms, 1);
        assert_eq!(s.unread_sms, 1);
    }
}
