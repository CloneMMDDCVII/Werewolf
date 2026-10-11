//! systemd's readiness and watchdog protocol (`sd_notify(3)`): one
//! datagram per state change to the socket named in `$NOTIFY_SOCKET`.
//! systemd sets that variable only for `Type=notify` units, so outside one
//! (and on Windows) every call does nothing. See `deploy/werewolf-bot.service`.

pub const SOCKET_ENV: &str = "NOTIFY_SOCKET";

/// Startup finished: polling has begun.
pub const READY: &str = "READY=1";
/// Still alive. Sent from the polling loop itself, so a hung poller stops
/// sending it and systemd restarts the bot after `WatchdogSec`.
pub const WATCHDOG: &str = "WATCHDOG=1";
/// Shutting down on purpose; games are winding down.
pub const STOPPING: &str = "STOPPING=1";

pub struct Notifier {
    target: Option<platform::Target>,
}

impl Notifier {
    pub fn from_env() -> Self {
        Self::new(std::env::var_os(SOCKET_ENV))
    }

    /// `socket` is the value of `$NOTIFY_SOCKET`, passed in so tests don't
    /// touch the process environment.
    pub fn new(socket: Option<std::ffi::OsString>) -> Self {
        let target = socket
            .filter(|s| !s.is_empty())
            .and_then(|s| match platform::Target::open(&s) {
                Ok(t) => Some(t),
                Err(e) => {
                    log::warn!("{SOCKET_ENV} is set but unusable: {e}");
                    None
                }
            });
        Notifier { target }
    }

    pub fn enabled(&self) -> bool {
        self.target.is_some()
    }

    /// Sends `state` (e.g. [`READY`]). A failure is logged, never fatal:
    /// the bot keeps running even if systemd stopped listening.
    pub fn notify(&self, state: &str) {
        if let Some(target) = &self.target {
            if let Err(e) = target.send(state.as_bytes()) {
                log::warn!("sd_notify {state}: {e}");
            }
        }
    }
}

#[cfg(not(unix))]
mod platform {
    use std::ffi::OsStr;
    use std::io;

    /// No notify socket off Unix; [`Target::open`] always fails.
    pub enum Target {}

    impl Target {
        pub fn open(_path: &OsStr) -> io::Result<Self> {
            Err(io::ErrorKind::Unsupported.into())
        }

        pub fn send(&self, _payload: &[u8]) -> io::Result<()> {
            match *self {}
        }
    }
}

#[cfg(unix)]
mod platform {
    use std::ffi::OsStr;
    use std::io;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::net::{SocketAddr, UnixDatagram};

    pub struct Target {
        socket: UnixDatagram,
        addr: SocketAddr,
    }

    impl Target {
        pub fn open(path: &OsStr) -> io::Result<Self> {
            Ok(Target {
                socket: UnixDatagram::unbound()?,
                addr: address(path.as_bytes())?,
            })
        }

        pub fn send(&self, payload: &[u8]) -> io::Result<()> {
            self.socket.send_to_addr(payload, &self.addr).map(|_| ())
        }
    }

    /// A leading `@` names a Linux abstract-namespace socket.
    #[cfg(target_os = "linux")]
    fn address(path: &[u8]) -> io::Result<SocketAddr> {
        use std::os::linux::net::SocketAddrExt;
        match path.strip_prefix(b"@") {
            Some(name) => SocketAddr::from_abstract_name(name),
            None => SocketAddr::from_pathname(OsStr::from_bytes(path)),
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn address(path: &[u8]) -> io::Result<SocketAddr> {
        SocketAddr::from_pathname(OsStr::from_bytes(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_a_socket_nothing_is_sent() {
        let n = Notifier::new(None);
        assert!(!n.enabled());
        n.notify(READY);
        n.notify(WATCHDOG);
        assert!(!Notifier::new(Some("".into())).enabled());
    }

    #[cfg(unix)]
    #[test]
    fn states_arrive_as_datagrams_on_the_socket() {
        let path = std::env::temp_dir().join(format!("wolf-notify-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let listener = std::os::unix::net::UnixDatagram::bind(&path).unwrap();
        let n = Notifier::new(Some(path.clone().into_os_string()));
        assert!(n.enabled());
        n.notify(READY);
        n.notify(WATCHDOG);
        let mut buf = [0u8; 64];
        let len = listener.recv(&mut buf).unwrap();
        assert_eq!(&buf[..len], READY.as_bytes());
        let len = listener.recv(&mut buf).unwrap();
        assert_eq!(&buf[..len], WATCHDOG.as_bytes());
        let _ = std::fs::remove_file(&path);
    }
}
