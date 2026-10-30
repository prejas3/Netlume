//! CCS/1 engine: WinDivert FLOW layer → (proto, local port) → pid; NETWORK layer → token-bucket shaping.
//! ponytail: #150 spike shapes one target pid; #151 adds stats, #152 per-app/global limits + block.
#![allow(dead_code)] // ponytail: wired into the app by #151

use std::{
    collections::{HashMap, VecDeque},
    ffi::CString,
    io,
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

    /// FLOW layer data: pid @16, local port @52, protocol @56 (host order).
    fn flow(&self) -> (u32, (u8, u16)) {
        let b: &[u8; 64] = unsafe { &*(self.data.as_ptr() as *const [u8; 64]) };
        let pid = u32::from_ne_bytes([b[16], b[17], b[18], b[19]]);
        (pid, (b[56], u16::from_ne_bytes([b[52], b[53]])))
    }
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

/// (protocol, local port) → pid.
/// ponytail: flows opened before start are unknown; #151 seeds them from GetExtendedTcp/UdpTable.
pub type Flows = Arc<Mutex<HashMap<(u8, u16), u32>>>;

pub fn track_flows(flows: Flows) -> io::Result<()> {
    let h = open("not loopback and (tcp or udp)", LAYER_FLOW, FLAG_SNIFF | FLAG_RECV_ONLY)?;
    thread::spawn(move || {
        let mut addr: Addr = unsafe { std::mem::zeroed() };
        while recv(h, &mut [], &mut 0, &mut addr) {
            let (pid, key) = addr.flow();
            let mut flows = flows.lock().unwrap();
            match addr.event() {
                EVENT_FLOW_ESTABLISHED => flows.insert(key, pid),
                EVENT_FLOW_DELETED => flows.remove(&key),
                _ => None,
            };
        }
    });
    Ok(())
}

/// (protocol, local port) of an IPv4/IPv6 TCP/UDP packet.
/// ponytail: IPv6 extension headers not walked; such packets pass unshaped.
fn packet_key(p: &[u8], outbound: bool) -> Option<(u8, u16)> {
    let (proto, off) = match p.first()? >> 4 {
        4 => (*p.get(9)?, (p[0] & 15) as usize * 4),
        6 => (*p.get(6)?, 40),
        _ => return None,
    };
    let ports = p.get(off..off + 4)?;
    let port = if outbound { [ports[0], ports[1]] } else { [ports[2], ports[3]] };
    Some((proto, u16::from_be_bytes(port)))
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
    queue: VecDeque<(Vec<u8>, Addr)>,
    bytes: usize,
    cap: usize,
}

/// Shapes the target pid's traffic; limits = [down, up] in bytes/s, None = unshaped.
/// ponytail: queue tail-drops past 0.5 s of data so TCP backs off instead of bloating RTT.
pub fn shape(flows: Flows, target: Arc<AtomicU32>, limits: [Option<u64>; 2]) -> io::Result<()> {
    let h = open("not loopback and (tcp or udp)", LAYER_NETWORK, 0)?;
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

    let rx = shared.clone();
    thread::spawn(move || {
        let mut buf = vec![0u8; 65_535];
        let mut addr: Addr = unsafe { std::mem::zeroed() };
        let mut len = 0u32;
        while recv(h, &mut buf, &mut len, &mut addr) {
            let packet = &buf[..len as usize];
            let pid = packet_key(packet, addr.outbound()).and_then(|key| flows.lock().unwrap().get(&key).copied());
            let mut lanes = rx.0.lock().unwrap();
            match &mut lanes[addr.outbound() as usize] {
                Some(lane) if pid.is_some_and(|pid| pid == target.load(Ordering::Relaxed)) => {
                    if lane.bytes + packet.len() <= lane.cap {
                        lane.bytes += packet.len();
                        lane.queue.push_back((packet.to_vec(), addr));
                        rx.1.notify_one();
                    }
                }
                _ => send(h, packet, &addr),
            }
        }
    });

    thread::spawn(move || {
        let mut lanes = shared.0.lock().unwrap();
        loop {
            let mut wait = None::<Duration>;
            for lane in lanes.iter_mut().flatten() {
                while let Some((packet, _)) = lane.queue.front() {
                    let w = lane.bucket.take(packet.len(), Instant::now());
                    if !w.is_zero() {
                        wait = Some(wait.map_or(w, |wait| wait.min(w)));
                        break;
                    }
                    let (packet, addr) = lane.queue.pop_front().unwrap();
                    lane.bytes -= packet.len();
                    send(h, &packet, &addr);
                }
            }
            lanes = match wait {
                Some(wait) => shared.1.wait_timeout(lanes, wait).unwrap().0,
                None => shared.1.wait(lanes).unwrap(),
            };
        }
    });
    Ok(())
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
    fn packet_key_reads_local_port() {
        let mut p = [0u8; 28];
        p[0] = 0x45;
        p[9] = 17;
        p[20..24].copy_from_slice(&[0x1f, 0x90, 0x00, 0x35]); // 8080 → 53
        assert_eq!(packet_key(&p, true), Some((17, 8080)));
        assert_eq!(packet_key(&p, false), Some((17, 53)));
        assert_eq!(packet_key(&p[..22], true), None);
    }

    /// Elevated: `src-tauri/windivert.ps1`, PATH += src-tauri/windivert, then
    /// `cargo test --lib spike -- --ignored --nocapture`.
    #[test]
    #[ignore = "needs admin, WinDivert files on PATH, internet"]
    fn spike_download_shaping() {
        const LIMIT: f64 = 1_000_000.0;
        const URL: &str = "https://speed.cloudflare.com/__down?bytes=20000000";
        let flows = Flows::default();
        let target = Arc::new(AtomicU32::new(0));
        track_flows(flows.clone()).unwrap();
        shape(flows.clone(), target.clone(), [Some(LIMIT as u64), None]).unwrap();

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

        let base = curl(false);
        let (cpu0, t0) = (cpu_seconds(), Instant::now());
        let shaped = curl(true);
        let cpu = (cpu_seconds() - cpu0) / t0.elapsed().as_secs_f64();
        let cores = thread::available_parallelism().map_or(1, |n| n.get());
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
