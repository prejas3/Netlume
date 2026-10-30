//! CCS/1 engine: WinDivert FLOW layer → 5-tuple → pid; NETWORK layer → per-app counts + token-bucket shaping.
//! ponytail: shapes one target pid; #152 brings per-app/global limits + block.

use crate::model::AppStat;
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    ffi::CString,
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    path::Path,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc, Condvar, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

const LAYER_NETWORK: i32 = 0;
const LAYER_FLOW: i32 = 2;
const FLAG_SNIFF: u64 = 1;
const FLAG_RECV_ONLY: u64 = 4;
const EVENT_FLOW_ESTABLISHED: u8 = 1;
const EVENT_FLOW_DELETED: u8 = 2;

/// WINDIVERT_ADDRESS (80 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Addr {
    timestamp: i64,
    bits: u32,
    reserved: u32,
    data: [u64; 8],
}

impl Addr {
    fn event(&self) -> u8 {
        (self.bits >> 8) as u8
    }

    fn outbound(&self) -> bool {
        self.bits >> 17 & 1 == 1
    }

    /// FLOW layer data, host order: pid @16, local addr @20, remote addr @36, ports @52/@54, protocol @56.
    fn flow(&self) -> (u32, Key) {
        let b: &[u8; 64] = unsafe { &*(self.data.as_ptr() as *const [u8; 64]) };
        let u16_at = |i: usize| u16::from_ne_bytes([b[i], b[i + 1]]);
        let pid = u32::from_ne_bytes([b[16], b[17], b[18], b[19]]);
        (pid, (b[56], flow_ip(&b[20..36]), u16_at(52), flow_ip(&b[36..52]), u16_at(54)))
    }
}

/// (protocol, local ip, local port, remote ip, remote port).
pub type Key = (u8, IpAddr, u16, IpAddr, u16);

/// WinDivert host-order IPv6 = words reversed, each native-endian; IPv4 arrives IPv4-mapped.
fn flow_ip(b: &[u8]) -> IpAddr {
    let mut ip = [0u8; 16];
    for i in 0..4 {
        let j = 12 - 4 * i;
        let word = u32::from_ne_bytes([b[j], b[j + 1], b[j + 2], b[j + 3]]);
        ip[4 * i..4 * i + 4].copy_from_slice(&word.to_be_bytes());
    }
    let ip = Ipv6Addr::from(ip);
    ip.to_ipv4_mapped().map_or(IpAddr::V6(ip), IpAddr::V4)
}

type Handle = isize;

/// WinDivert.dll loaded at runtime, so the app (and UI dev) runs without it; open() then errs.
struct WinDivert {
    open: unsafe extern "C" fn(*const i8, i32, i16, u64) -> Handle,
    recv: unsafe extern "C" fn(Handle, *mut u8, u32, *mut u32, *mut Addr) -> i32,
    send: unsafe extern "C" fn(Handle, *const u8, u32, *mut u32, *const Addr) -> i32,
}

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryW(name: *const u16) -> isize;
    fn GetProcAddress(module: isize, name: *const u8) -> usize;
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> isize;
    fn QueryFullProcessImageNameW(h: isize, flags: u32, buf: *mut u16, len: *mut u32) -> i32;
    fn CloseHandle(h: isize) -> i32;
}

fn windivert() -> io::Result<&'static WinDivert> {
    static DLL: std::sync::OnceLock<Option<WinDivert>> = std::sync::OnceLock::new();
    DLL.get_or_init(|| unsafe {
        let name: Vec<u16> = "WinDivert.dll\0".encode_utf16().collect();
        let module = LoadLibraryW(name.as_ptr());
        let proc = |name: &[u8]| GetProcAddress(module, name.as_ptr());
        let (open, recv, send) = (proc(b"WinDivertOpen\0"), proc(b"WinDivertRecv\0"), proc(b"WinDivertSend\0"));
        (module != 0 && open != 0 && recv != 0 && send != 0).then(|| WinDivert {
            open: std::mem::transmute(open),
            recv: std::mem::transmute(recv),
            send: std::mem::transmute(send),
        })
    })
    .as_ref()
    .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "WinDivert.dll not found"))
}

fn open(filter: &str, layer: i32, flags: u64) -> io::Result<Handle> {
    let filter = CString::new(filter).expect("filter has no NUL");
    let h = unsafe { (windivert()?.open)(filter.as_ptr(), layer, 0, flags) };
    if h == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(h)
}

/// Blocking; false once the handle fails or closes.
fn recv(h: Handle, buf: &mut [u8], len: &mut u32, addr: &mut Addr) -> bool {
    let wd = windivert().expect("opened");
    unsafe { (wd.recv)(h, buf.as_mut_ptr(), buf.len() as u32, len, addr) != 0 }
}

fn send(h: Handle, packet: &[u8], addr: &Addr) {
    let wd = windivert().expect("opened");
    unsafe { (wd.send)(h, packet.as_ptr(), packet.len() as u32, std::ptr::null_mut(), addr) };
}

pub fn exe_path(pid: u32) -> Option<String> {
    unsafe {
        let h = OpenProcess(0x1000, 0, pid); // PROCESS_QUERY_LIMITED_INFORMATION
        if h == 0 {
            return None;
        }
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut len);
        CloseHandle(h);
        (ok != 0).then(|| String::from_utf16_lossy(&buf[..len as usize]))
    }
}

#[link(name = "iphlpapi")]
extern "system" {
    fn GetExtendedTcpTable(table: *mut u8, size: *mut u32, order: i32, af: u32, class: u32, reserved: u32) -> u32;
    fn GetExtendedUdpTable(table: *mut u8, size: *mut u32, order: i32, af: u32, class: u32, reserved: u32) -> u32;
}

/// (protocol, local port) → pid from the OS TCP/UDP owner tables (IPv4 + IPv6).
fn port_owners() -> HashMap<(u8, u16), u32> {
    type Get = unsafe extern "system" fn(*mut u8, *mut u32, i32, u32, u32, u32) -> u32;
    let mut owners = HashMap::new();
    // (proto, get, AF, class, row size, port @, pid @): MIB_{TCP,TCP6,UDP,UDP6}ROW_OWNER_PID.
    let tables: [(u8, Get, u32, u32, usize, usize, usize); 4] = [
        (6, GetExtendedTcpTable, 2, 5, 24, 8, 20),
        (6, GetExtendedTcpTable, 23, 5, 56, 20, 52),
        (17, GetExtendedUdpTable, 2, 1, 12, 4, 8),
        (17, GetExtendedUdpTable, 23, 1, 28, 20, 24),
    ];
    for (proto, get, af, class, row, port_at, pid_at) in tables {
        let mut size = 0u32;
        unsafe { get(std::ptr::null_mut(), &mut size, 0, af, class, 0) };
        size += 4096; // room for sockets opened between the two calls
        let mut buf = vec![0u32; size as usize / 4];
        if unsafe { get(buf.as_mut_ptr().cast(), &mut size, 0, af, class, 0) } != 0 {
            continue;
        }
        let b: Vec<u8> = buf.iter().flat_map(|w| w.to_ne_bytes()).collect();
        for r in (0..buf[0] as usize).map(|i| 4 + i * row).take_while(|r| r + row <= b.len()) {
            let port = u16::from_be_bytes([b[r + port_at], b[r + port_at + 1]]);
            let pid = u32::from_ne_bytes([b[r + pid_at], b[r + pid_at + 1], b[r + pid_at + 2], b[r + pid_at + 3]]);
            owners.insert((proto, port), pid);
        }
    }
    owners
}

/// Live engine state shared by its threads and the app.
#[derive(Default)]
pub struct Engine {
    /// 5-tuple → pid from FLOW events.
    pub flows: Mutex<HashMap<Key, u32>>,
    /// Fallback for flows opened before start, refreshed each tick.
    /// ponytail: port-only, so a UDP port shared by two apps can misattribute; FLOW events win.
    owners: Mutex<HashMap<(u8, u16), u32>>,
    /// pid → [down, up] bytes passed since the last tick; pid 0 = unknown.
    traffic: Mutex<HashMap<u32, [u64; 2]>>,
    /// ponytail: one shaped pid until #152 brings per-app limits.
    pub target: AtomicU32,
}

impl Engine {
    fn pid(&self, key: Option<Key>) -> u32 {
        let Some(key) = key else { return 0 };
        let flow = self.flows.lock().unwrap().get(&key).copied();
        flow.or_else(|| self.owners.lock().unwrap().get(&(key.0, key.2)).copied()).unwrap_or(0)
    }

    /// Reinjects a packet and counts it for its pid (counted on send, so shaper drops don't count).
    fn pass(&self, h: Handle, packet: &[u8], addr: &Addr, pid: u32) {
        self.traffic.lock().unwrap().entry(pid).or_default()[addr.outbound() as usize] += packet.len() as u64;
        send(h, packet, addr);
    }

    /// Call once a second: rates = bytes since the last tick, totals for the session, per exe path.
    pub fn tick(&self, apps: &mut BTreeMap<String, AppStat>) -> Vec<AppStat> {
        *self.owners.lock().unwrap() = port_owners();
        let traffic = std::mem::take(&mut *self.traffic.lock().unwrap());
        let by_path = traffic
            .into_iter()
            .map(|(pid, bytes)| (exe_path(pid).unwrap_or_else(|| SYSTEM.into()), bytes));
        merge(apps, by_path)
    }
}

/// Path of the bucket for System (pid 4), unknown flows and exited processes.
const SYSTEM: &str = "System";

fn merge(apps: &mut BTreeMap<String, AppStat>, traffic: impl Iterator<Item = (String, [u64; 2])>) -> Vec<AppStat> {
    for app in apps.values_mut() {
        app.down_bps = 0;
        app.up_bps = 0;
    }
    for (path, [down, up]) in traffic {
        let app = apps.entry(path.clone()).or_insert_with(|| AppStat {
            id: path.clone(),
            name: match path.as_str() {
                SYSTEM => "System / unknown".into(),
                _ => Path::new(&path).file_stem().map_or(path.clone(), |s| s.to_string_lossy().into()),
            },
            path,
            down_bps: 0,
            up_bps: 0,
            down_total: 0,
            up_total: 0,
            limit: None,
            blocked: false,
        });
        app.down_bps += down;
        app.up_bps += up;
        app.down_total += down;
        app.up_total += up;
    }
    apps.values().cloned().collect()
}

fn on_flow(flows: &mut HashMap<Key, u32>, addr: &Addr) {
    let (pid, key) = addr.flow();
    match addr.event() {
        EVENT_FLOW_ESTABLISHED => flows.insert(key, pid),
        EVENT_FLOW_DELETED => flows.remove(&key),
        _ => None,
    };
}

/// 5-tuple of an IPv4/IPv6 TCP/UDP packet, local side first.
/// ponytail: IPv6 extension headers not walked; such packets pass unshaped.
fn packet_key(p: &[u8], outbound: bool) -> Option<Key> {
    let (proto, off, src, dst): (u8, usize, IpAddr, IpAddr) = match p.first()? >> 4 {
        4 => {
            let (a, b): ([u8; 4], [u8; 4]) = (p.get(12..16)?.try_into().ok()?, p.get(16..20)?.try_into().ok()?);
            (p[9], (p[0] & 15) as usize * 4, Ipv4Addr::from(a).into(), Ipv4Addr::from(b).into())
        }
        6 => {
            let (a, b): ([u8; 16], [u8; 16]) = (p.get(8..24)?.try_into().ok()?, p.get(24..40)?.try_into().ok()?);
            (p[6], 40, Ipv6Addr::from(a).into(), Ipv6Addr::from(b).into())
        }
        _ => return None,
    };
    let ports = p.get(off..off + 4)?;
    let (sport, dport) = (u16::from_be_bytes([ports[0], ports[1]]), u16::from_be_bytes([ports[2], ports[3]]));
    Some(if outbound { (proto, src, sport, dst, dport) } else { (proto, dst, dport, src, sport) })
}

/// Debt token bucket: a packet goes whenever tokens >= 0, so packets larger than the burst never stall.
pub struct Bucket {
    rate: f64,
    burst: f64,
    tokens: f64,
    last: Instant,
}

impl Bucket {
    pub fn new(rate: u64, now: Instant) -> Self {
        let rate = rate as f64;
        let burst = (rate / 10.0).max(16_384.0);
        Self { rate, burst, tokens: burst, last: now }
    }

    /// Zero = send now (tokens taken); otherwise how long until it may go.
    pub fn take(&mut self, len: usize, now: Instant) -> Duration {
        let elapsed = now.saturating_duration_since(self.last).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.rate).min(self.burst);
        self.last = now;
        if self.tokens < 0.0 {
            return Duration::from_secs_f64(-self.tokens / self.rate).max(Duration::from_nanos(1));
        }
        self.tokens -= len as f64;
        Duration::ZERO
    }
}

struct Lane {
    bucket: Bucket,
    queue: VecDeque<(Vec<u8>, Addr, u32)>,
    bytes: usize,
    cap: usize,
}

/// Starts attribution, counting and shaping of `target`; limits = [down, up] bytes/s, None = unshaped.
/// ponytail: all non-loopback TCP/UDP is diverted through this process; it exiting releases the driver.
/// ponytail: queue tail-drops past 0.5 s of data so TCP backs off instead of bloating RTT.
pub fn start(limits: [Option<u64>; 2]) -> io::Result<Arc<Engine>> {
    let engine = Arc::new(Engine::default());
    let fh = open("not loopback and (tcp or udp)", LAYER_FLOW, FLAG_SNIFF | FLAG_RECV_ONLY)?;
    *engine.owners.lock().unwrap() = port_owners();
    let h = open("not loopback and (tcp or udp)", LAYER_NETWORK, 0)?;

    let flows = engine.clone();
    thread::spawn(move || {
        let mut addr: Addr = unsafe { std::mem::zeroed() };
        while recv(fh, &mut [], &mut 0, &mut addr) {
            on_flow(&mut flows.flows.lock().unwrap(), &addr);
        }
    });

    let now = Instant::now();
    let lanes = limits.map(|limit| {
        limit.map(|rate| Lane {
            bucket: Bucket::new(rate, now),
            queue: VecDeque::new(),
            bytes: 0,
            cap: (rate as usize / 2).max(65_536),
        })
    });
    let shared = Arc::new((Mutex::new(lanes), Condvar::new()));

    let (rx, rx_engine) = (shared.clone(), engine.clone());
    thread::spawn(move || {
        let engine = rx_engine;
        let mut buf = vec![0u8; 65_535];
        let mut addr: Addr = unsafe { std::mem::zeroed() };
        let mut len = 0u32;
        while recv(h, &mut buf, &mut len, &mut addr) {
            let packet = &buf[..len as usize];
            let pid = engine.pid(packet_key(packet, addr.outbound()));
            let mut lanes = rx.0.lock().unwrap();
            match &mut lanes[addr.outbound() as usize] {
                Some(lane) if pid != 0 && pid == engine.target.load(Ordering::Relaxed) => {
                    if lane.bytes + packet.len() <= lane.cap {
                        lane.bytes += packet.len();
                        lane.queue.push_back((packet.to_vec(), addr, pid));
                        rx.1.notify_one();
                    }
                }
                _ => engine.pass(h, packet, &addr, pid),
            }
        }
    });

    let tx_engine = engine.clone();
    thread::spawn(move || {
        let engine = tx_engine;
        let mut lanes = shared.0.lock().unwrap();
        loop {
            let mut wait = None::<Duration>;
            for lane in lanes.iter_mut().flatten() {
                while let Some((packet, ..)) = lane.queue.front() {
                    let w = lane.bucket.take(packet.len(), Instant::now());
                    if !w.is_zero() {
                        wait = Some(wait.map_or(w, |wait| wait.min(w)));
                        break;
                    }
                    let (packet, addr, pid) = lane.queue.pop_front().unwrap();
                    lane.bytes -= packet.len();
                    engine.pass(h, &packet, &addr, pid);
                }
            }
            lanes = match wait {
                Some(wait) => shared.1.wait_timeout(lanes, wait).unwrap().0,
                None => shared.1.wait(lanes).unwrap(),
            };
        }
    });
    Ok(engine)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_holds_rate_over_time() {
        let t0 = Instant::now();
        let mut bucket = Bucket::new(100_000, t0);
        let (mut t, mut sent) = (t0, 0usize);
        while t < t0 + Duration::from_secs(10) {
            match bucket.take(1500, t) {
                w if w.is_zero() => sent += 1500,
                w => t += w,
            }
        }
        // 10 s at 100 kB/s plus one burst (16 KiB) and one packet of debt.
        let expected = 1_000_000.0 + 16_384.0 + 1500.0;
        assert!((sent as f64 - expected).abs() <= 1500.0, "sent {sent}");
    }

    #[test]
    fn bucket_never_stalls_on_jumbo_packet() {
        let t0 = Instant::now();
        let mut bucket = Bucket::new(1000, t0);
        assert!(bucket.take(64_000, t0).is_zero());
        let w = bucket.take(1, t0);
        assert!((w.as_secs_f64() - (64_000.0 - 16_384.0) / 1000.0).abs() < 1e-6);
        assert!(bucket.take(1, t0 + w).is_zero());
    }

    #[test]
    fn merge_sums_by_path_and_resets_rates() {
        let mut apps = BTreeMap::new();
        let exe = r"C:\Apps\Browser.exe".to_string();
        let tick1 = [(exe.clone(), [100, 10]), (exe.clone(), [50, 5]), (SYSTEM.into(), [7, 0])];
        let stats = merge(&mut apps, tick1.into_iter());
        assert_eq!(stats.len(), 2);
        let browser = &apps[&exe];
        assert_eq!((browser.name.as_str(), browser.down_bps, browser.up_bps), ("Browser", 150, 15));
        assert_eq!(apps[SYSTEM].name, "System / unknown");

        merge(&mut apps, [(exe.clone(), [1, 0])].into_iter());
        let browser = &apps[&exe];
        assert_eq!((browser.down_bps, browser.down_total, browser.up_bps, browser.up_total), (1, 151, 0, 15));
        assert_eq!((apps[SYSTEM].down_bps, apps[SYSTEM].down_total), (0, 7));
    }

    #[test]
    fn port_owners_reads_os_tables() {
        let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = socket.local_addr().unwrap().port();
        assert_eq!(port_owners().get(&(17, port)), Some(&std::process::id()));
    }

    /// FLOW-layer Addr in WinDivert's host-order, IPv4-mapped form.
    fn flow_addr(event: u8, pid: u32, local: [u8; 4], lport: u16, remote: [u8; 4], rport: u16) -> Addr {
        let mut addr: Addr = unsafe { std::mem::zeroed() };
        addr.bits = (event as u32) << 8;
        let b: &mut [u8; 64] = unsafe { &mut *(addr.data.as_mut_ptr() as *mut [u8; 64]) };
        b[16..20].copy_from_slice(&pid.to_ne_bytes());
        for (at, ip) in [(20, local), (36, remote)] {
            b[at..at + 4].copy_from_slice(&u32::from_be_bytes(ip).to_ne_bytes());
            b[at + 4..at + 8].copy_from_slice(&0xffffu32.to_ne_bytes());
        }
        b[52..54].copy_from_slice(&lport.to_ne_bytes());
        b[54..56].copy_from_slice(&rport.to_ne_bytes());
        b[56] = 6;
        addr
    }

    fn tcp4(src: [u8; 4], sport: u16, dst: [u8; 4], dport: u16) -> [u8; 24] {
        let mut p = [0u8; 24];
        p[0] = 0x45;
        p[9] = 6;
        p[12..16].copy_from_slice(&src);
        p[16..20].copy_from_slice(&dst);
        p[20..22].copy_from_slice(&sport.to_be_bytes());
        p[22..24].copy_from_slice(&dport.to_be_bytes());
        p
    }

    #[test]
    fn flows_keep_remotes_on_one_local_port_apart() {
        let (me, a, b) = ([10, 0, 0, 2], [1, 1, 1, 1], [8, 8, 8, 8]);
        let mut flows = HashMap::new();
        on_flow(&mut flows, &flow_addr(EVENT_FLOW_ESTABLISHED, 100, me, 5000, a, 443));
        on_flow(&mut flows, &flow_addr(EVENT_FLOW_ESTABLISHED, 200, me, 5000, b, 443));
        let pid = |p: &[u8], out| packet_key(p, out).and_then(|k| flows.get(&k).copied());
        assert_eq!(pid(&tcp4(a, 443, me, 5000), false), Some(100));
        assert_eq!(pid(&tcp4(me, 5000, b, 443), true), Some(200));

        on_flow(&mut flows, &flow_addr(EVENT_FLOW_DELETED, 100, me, 5000, a, 443));
        assert_eq!(flows.len(), 1);
        assert_eq!(packet_key(&tcp4(b, 443, me, 5000), false).and_then(|k| flows.get(&k).copied()), Some(200));
        assert_eq!(packet_key(&tcp4(a, 443, me, 5000)[..22], false), None);
    }

    #[test]
    fn flow_ip_reads_host_order_ipv6() {
        let mut b = [0u8; 16];
        for (i, word) in [1u32, 0, 0, 0x2001_0db8].iter().enumerate() {
            b[4 * i..4 * i + 4].copy_from_slice(&word.to_ne_bytes());
        }
        assert_eq!(flow_ip(&b), "2001:db8::1".parse::<IpAddr>().unwrap());
    }

    /// Elevated: `src-tauri/windivert.ps1`, PATH += src-tauri/windivert, then
    /// `cargo test --lib spike -- --ignored --nocapture`.
    #[test]
    #[ignore = "needs admin, WinDivert files on PATH, internet"]
    fn spike_download_shaping() {
        const LIMIT: f64 = 1_000_000.0;
        const URL: &str = "https://speed.cloudflare.com/__down?bytes=20000000";
        let engine = start([Some(LIMIT as u64), None]).unwrap();
        let (flows, target) = (&engine.flows, &engine.target);
        let mut apps = BTreeMap::new();

        // Idle cost + what the monitor sees over 10 s of normal background traffic.
        let (cpu0, t0) = (cpu_seconds(), Instant::now());
        thread::sleep(Duration::from_secs(10));
        let idle = (cpu_seconds() - cpu0) / t0.elapsed().as_secs_f64();
        let seen = engine.tick(&mut apps);
        for app in &seen {
            println!("IDLE {:>10} B down {:>10} B up  {}", app.down_total, app.up_total, app.path);
        }

        let curl = |shaped: bool| {
            let child = std::process::Command::new("curl.exe")
                .args(["-s", "-o", "NUL", "-w", "%{speed_download} %{time_total} %{time_starttransfer}", URL])
                .stdout(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            let pid = child.id();
            if shaped {
                target.store(pid, Ordering::Relaxed);
            }
            let deadline = Instant::now() + Duration::from_secs(10);
            while !flows.lock().unwrap().values().any(|&p| p == pid) && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(5));
            }
            let attributed = flows.lock().unwrap().values().any(|&p| p == pid);
            let path = exe_path(pid);
            let out = String::from_utf8(child.wait_with_output().unwrap().stdout).unwrap();
            let v: Vec<f64> = out.split_whitespace().map(|s| s.parse().unwrap()).collect();
            (v[0], v[1], v[2], attributed, path)
        };

        let (cpu0, t0) = (cpu_seconds(), Instant::now());
        let base = curl(false);
        let full = (cpu_seconds() - cpu0) / t0.elapsed().as_secs_f64();
        let curl_total = engine
            .tick(&mut apps)
            .into_iter()
            .find(|app| app.path.to_lowercase().ends_with("curl.exe"))
            .map_or(0, |app| app.down_total);
        let (cpu0, t0) = (cpu_seconds(), Instant::now());
        let shaped = curl(true);
        let cpu = (cpu_seconds() - cpu0) / t0.elapsed().as_secs_f64();
        let cores = thread::available_parallelism().map_or(1, |n| n.get());
        println!(
            "MONITOR curl counted {curl_total} B down for a 20,000,000 B body; CPU idle {:.2}% / full speed {:.1}% of one core",
            idle * 100.0,
            full * 100.0,
        );
        println!(
            "SPIKE base {:.0} B/s; shaped {:.0} B/s = {:+.1}% vs {LIMIT} limit, total {:.1}s, ttfb {:.2}s; \
             attributed {} {:?}; engine CPU {:.1}% of one core ({:.2}% of {cores})",
            base.0,
            shaped.0,
            (shaped.0 / LIMIT - 1.0) * 100.0,
            shaped.1,
            shaped.2,
            shaped.3,
            shaped.4,
            cpu * 100.0,
            cpu * 100.0 / cores as f64,
        );
        assert!(shaped.3, "curl flow not attributed");
        assert!((20_000_000..22_000_000).contains(&curl_total), "monitor miscounted curl");
        assert!((shaped.0 / LIMIT - 1.0).abs() <= 0.2, "rate off by more than 20%");
    }

    fn cpu_seconds() -> f64 {
        #[link(name = "kernel32")]
        extern "system" {
            fn GetProcessTimes(h: isize, c: *mut u64, e: *mut u64, k: *mut u64, u: *mut u64) -> i32;
        }
        let (mut c, mut e, mut k, mut u) = (0, 0, 0, 0);
        unsafe { GetProcessTimes(-1, &mut c, &mut e, &mut k, &mut u) };
        (k + u) as f64 / 1e7
    }
}
