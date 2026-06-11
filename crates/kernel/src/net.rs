//! TCP/IP via smoltcp (todo 16 Tier 7).
//!
//! Two interfaces — a `Loopback` (127.0.0.1/8) and, when qemu provides
//! a virtio-net device, an ethernet `Interface` (10.0.2.15/24, SLIRP).
//!
//! CRITICAL design point: each interface owns its OWN `SocketSet`.
//! smoltcp's `socket_egress` walks every socket in the set it is
//! handed with no route/ownership filter, and the loopback
//! (`Medium::Ip`) does *no* route lookup in `dispatch_ip` — so a single
//! shared set lets the loopback poll "successfully" emit an
//! eth-bound socket's data segment into the loopback void, then
//! advance the TCP sequence number (smoltcp commits seq only after a
//! successful emit). The eth interface then thinks the data is already
//! in flight and sends a bare ACK. Per-interface sets make each socket
//! reachable only by the interface that actually routes it.
//!
//! Shape: one global `NetStack` behind a SpinLock. Syscalls lock it,
//! operate on their socket via a `NetHandle` (interface tag + smoltcp
//! handle), then `kick()` the net task; the net task owns the poll
//! loop. Blocking syscalls park on smoltcp's per-socket send/recv
//! wakers, which are plain `core::task::Waker`s — exactly what our
//! executor hands out.

use alloc::vec;
use alloc::vec::Vec;

use core::future::Future;
use core::pin::Pin;
use core::sync::atomic::{AtomicU16, Ordering};
use core::task::{Context, Poll};

use smoltcp::iface::{Config, Interface, SocketSet};
use smoltcp::phy::{Loopback, Medium};
use smoltcp::socket::tcp;
use smoltcp::time::Instant;
use smoltcp::wire::{HardwareAddress, IpAddress, IpCidr, IpEndpoint, Ipv4Address};

use hal::Hal;

use crate::arch::{Arch, TIMER_INTERVAL};
use crate::sync::SpinLock;
use crate::wait::WakerCell;

/// Per-direction TCP buffer size. Heap-backed (the allocator frees).
const TCP_BUF: usize = 16 * 1024;

/// A socket plus which interface's set it lives in. Stored in
/// `File::Socket`'s Tcp/TcpListening states.
#[derive(Clone, Copy)]
pub struct NetHandle {
    /// true → the eth set, false → the loopback set.
    eth: bool,
    handle: smoltcp::iface::SocketHandle,
}

pub struct NetStack {
    lo: Loopback,
    lo_iface: Interface,
    lo_sockets: SocketSet<'static>,
    /// virtio-net device + its interface + its OWN socket set —
    /// present only when qemu gave us the NIC (the driver probes).
    eth: Option<(crate::driver::virtio_net::NetDev, Interface, SocketSet<'static>)>,
}

impl NetStack {
    /// The socket for `h`. Panics only on a logic error (an eth handle
    /// can exist only if eth was present at creation, and eth is never
    /// torn down).
    pub fn sock(&mut self, h: NetHandle) -> &mut tcp::Socket<'static> {
        let set = if h.eth {
            &mut self.eth.as_mut().expect("eth handle without eth").2
        } else {
            &mut self.lo_sockets
        };
        set.get_mut::<tcp::Socket>(h.handle)
    }

    pub fn remove(&mut self, h: NetHandle) {
        if h.eth {
            if let Some(e) = self.eth.as_mut() {
                e.2.remove(h.handle);
            }
        } else {
            self.lo_sockets.remove(h.handle);
        }
    }

    /// Pick the interface that routes `addr`: loopback for 127/8, the
    /// NIC for everything else.
    fn is_eth_addr(&self, addr: Ipv4Address) -> bool {
        addr.octets()[0] != 127
    }

    fn fresh(&mut self, eth: bool) -> Option<NetHandle> {
        let rx = tcp::SocketBuffer::new(vec![0u8; TCP_BUF]);
        let tx = tcp::SocketBuffer::new(vec![0u8; TCP_BUF]);
        let sock = tcp::Socket::new(rx, tx);
        let handle = if eth {
            self.eth.as_mut()?.2.add(sock)
        } else {
            self.lo_sockets.add(sock)
        };
        Some(NetHandle { eth, handle })
    }

    /// Arm a listening socket bound to `addr:port`. `0.0.0.0` listens
    /// on the eth interface when present (the externally-reachable
    /// one), else loopback; an explicit 127.x address forces loopback.
    pub fn listen(&mut self, addr: Ipv4Address, port: u16) -> Option<NetHandle> {
        let eth = if addr.octets()[0] == 127 {
            false
        } else if addr == Ipv4Address::UNSPECIFIED {
            self.eth.is_some()
        } else {
            true
        };
        let h = self.fresh(eth)?;
        if self.sock(h).listen(port).is_err() {
            self.remove(h);
            return None;
        }
        Some(h)
    }

    /// Re-arm a fresh listener on the same interface + port (accept()
    /// hands the old listening socket out as the connection).
    pub fn relisten(&mut self, like: NetHandle, port: u16) -> Option<NetHandle> {
        let h = self.fresh(like.eth)?;
        let _ = self.sock(h).listen(port);
        Some(h)
    }

    /// Begin an active connect to `addr:port` from local port `local`.
    /// Adds the socket to the routing interface's set and drives the
    /// SYN. Returns None if the NIC is needed but absent, or connect
    /// is rejected.
    pub fn connect(
        &mut self,
        addr: Ipv4Address,
        port: u16,
        local: u16,
    ) -> Option<NetHandle> {
        let eth = self.is_eth_addr(addr);
        let h = self.fresh(eth)?;
        let remote = endpoint(addr, port);
        let ok = if eth {
            let (_, iface, set) = self.eth.as_mut().unwrap();
            set.get_mut::<tcp::Socket>(h.handle)
                .connect(iface.context(), remote, local)
                .is_ok()
        } else {
            self.lo_sockets
                .get_mut::<tcp::Socket>(h.handle)
                .connect(self.lo_iface.context(), remote, local)
                .is_ok()
        };
        if !ok {
            self.remove(h);
            return None;
        }
        Some(h)
    }
}

static NET: SpinLock<Option<NetStack>> = SpinLock::new(None);
/// Single waiter: the net task.
static NET_KICK: WakerCell = WakerCell::new();
/// Handles whose last fd dropped — the net task closes them
/// gracefully and removes them once fully Closed.
static GC: SpinLock<Vec<NetHandle>> = SpinLock::new(Vec::new());

fn now() -> Instant {
    // TIMER_INTERVAL ticks ≈ one 100 ms timer period on riscv (the
    // aarch64 counter runs faster, skewing "ms" — fine: smoltcp only
    // needs monotonic time; timeouts stretch, correctness holds).
    Instant::from_millis((Arch::now_ticks() / (TIMER_INTERVAL / 100)) as i64)
}

/// Wake the net task so it re-polls soon.
pub fn kick() {
    NET_KICK.wake();
}

/// Build the loopback interface (+ eth if present). Called once from
/// bringup (needs the heap, the NIC already probed).
pub fn init() {
    let mut lo = Loopback::new(Medium::Ip);
    let config = Config::new(HardwareAddress::Ip);
    let mut lo_iface = Interface::new(config, &mut lo, now());
    lo_iface.update_ip_addrs(|addrs| {
        addrs
            .push(IpCidr::new(IpAddress::v4(127, 0, 0, 1), 8))
            .expect("lo addr");
    });

    // Ethernet via virtio-net, if qemu gave us one. SLIRP defaults:
    // we are 10.0.2.15/24, the gateway (and host alias) is 10.0.2.2.
    let eth = if crate::driver::virtio_net::present() {
        let mac = crate::driver::virtio_net::mac();
        let mut dev = crate::driver::virtio_net::NetDev;
        let mut cfg = Config::new(HardwareAddress::Ethernet(
            smoltcp::wire::EthernetAddress(mac),
        ));
        cfg.random_seed = 0x4242_4242_4242_4242;
        let mut iface = Interface::new(cfg, &mut dev, now());
        iface.update_ip_addrs(|addrs| {
            addrs
                .push(IpCidr::new(IpAddress::v4(10, 0, 2, 15), 24))
                .expect("eth addr");
        });
        iface
            .routes_mut()
            .add_default_ipv4_route(Ipv4Address::new(10, 0, 2, 2))
            .expect("default route");
        crate::println!("net: eth up (10.0.2.15/24 gw 10.0.2.2)");
        Some((dev, iface, SocketSet::new(Vec::new())))
    } else {
        None
    };

    *NET.lock() = Some(NetStack {
        lo,
        lo_iface,
        lo_sockets: SocketSet::new(Vec::new()),
        eth,
    });
}

/// Run `f` with the stack locked. Panics if `init` hasn't run —
/// syscalls can't arrive before bringup finishes.
pub fn with<R>(f: impl FnOnce(&mut NetStack) -> R) -> R {
    let mut g = NET.lock();
    f(g.as_mut().expect("net not initialized"))
}

/// Allocate an ephemeral local port.
pub fn ephemeral_port() -> u16 {
    static NEXT: AtomicU16 = AtomicU16::new(49152);
    let p = NEXT.fetch_add(1, Ordering::Relaxed);
    if p == 0 {
        49152
    } else {
        p
    }
}

/// Queue a handle for graceful teardown (called from `Drop` — no
/// awaits, just lock/push/kick).
pub fn sock_drop(handle: NetHandle) {
    GC.lock().push(handle);
    kick();
}

/// The kernel net task: polls each interface (over its own set)
/// whenever kicked or at smoltcp's deadline, and garbage-collects
/// dropped connections.
pub async fn net_task() {
    loop {
        let delay_ms = with(|stack| {
            // Reap dropped sockets: close, then remove once inert.
            let mut gc = GC.lock();
            gc.retain(|&h| {
                let active = {
                    let s = stack.sock(h);
                    if s.is_active() {
                        s.close();
                        true
                    } else {
                        false
                    }
                };
                if active {
                    true
                } else {
                    stack.remove(h);
                    false
                }
            });
            drop(gc);

            let t = now();
            stack
                .lo_iface
                .poll(t, &mut stack.lo, &mut stack.lo_sockets);
            let mut delay = stack.lo_iface.poll_delay(t, &stack.lo_sockets);
            if let Some((dev, iface, set)) = stack.eth.as_mut() {
                iface.poll(t, dev, set);
                let d2 = iface.poll_delay(t, set);
                delay = match (delay, d2) {
                    (Some(a), Some(b)) => Some(a.min(b)),
                    (a, None) => a,
                    (None, b) => b,
                };
            }
            delay.map(|d| d.total_millis())
        });
        // Always re-poll within ~20 ms even if smoltcp asked for no
        // deadline: a NIC RX completion that didn't deliver an IRQ
        // (or arrived between poll and park) still gets serviced.
        // (Loopback-only runs never hit this — kicks drive it.)
        let cap = Arch::now_ticks() + TIMER_INTERVAL / 5;
        let deadline = Some(match delay_ms {
            Some(ms) => (Arch::now_ticks()
                + (ms as u64).max(1) * (TIMER_INTERVAL / 100))
            .min(cap),
            None => cap,
        });
        NetWait { deadline, armed: false }.await;
    }
}

/// Two-phase parking future (same pattern as poll's PollWait): first
/// poll registers on the kick cell + optional timer and parks; any
/// wake re-polls it to Ready, and the loop above re-runs the stack.
struct NetWait {
    deadline: Option<u64>,
    armed: bool,
}

impl Future for NetWait {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.armed {
            return Poll::Ready(());
        }
        self.armed = true;
        NET_KICK.register(cx.waker());
        if let Some(d) = self.deadline {
            crate::time::add_timer(d, cx.waker().clone());
        }
        Poll::Pending
    }
}

/// Parse "a.b.c.d:port" (or ":port" / "port" for binds) into an
/// (address, port). Returns None on malformed input.
pub fn parse_endpoint(s: &str) -> Option<(Ipv4Address, u16)> {
    let (host, port) = match s.rfind(':') {
        Some(i) => (&s[..i], &s[i + 1..]),
        None => ("", s),
    };
    let port: u16 = port.parse().ok()?;
    if port == 0 {
        return None;
    }
    let addr = if host.is_empty() || host == "0.0.0.0" {
        Ipv4Address::UNSPECIFIED
    } else {
        let mut oct = [0u8; 4];
        let mut n = 0;
        for part in host.split('.') {
            if n >= 4 {
                return None;
            }
            oct[n] = part.parse().ok()?;
            n += 1;
        }
        if n != 4 {
            return None;
        }
        Ipv4Address::new(oct[0], oct[1], oct[2], oct[3])
    };
    Some((addr, port))
}

pub fn endpoint(addr: Ipv4Address, port: u16) -> IpEndpoint {
    IpEndpoint::new(IpAddress::Ipv4(addr), port)
}
