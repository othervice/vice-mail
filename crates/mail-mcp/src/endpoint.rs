//! Where the app listens for the helper (decision 0015): a Unix domain socket on macOS, a
//! named pipe on Windows. Never a network socket.
//!
//! macOS: the socket lies alone in its own directory (`<data dir>/mcp/`), which gets mode 0700;
//! the socket gets 0600. Only the user (and root) can connect. A stale socket from a crashed run
//! is removed; a socket someone is listening on is not touched. The host also checks the peer's
//! user id on every connection.
//!
//! Windows: a pipe `\\.\pipe\mail-mcp-<128 random bits>` created with `first_pipe_instance`
//! (fails if the name exists, so nobody can sit on it) and `reject_remote_clients`. Setting an
//! own DACL needs `unsafe` (tokio's `create_with_security_attributes_raw`), so the pipe keeps the
//! default one (other users may open it for reading only); the unguessable name, the
//! authentication of both sides (`crate::ipc`) and the fact that the app writes nothing before
//! the client has proven its key make that harmless.

use std::io;
use std::path::Path;

use tokio::io::{AsyncRead, AsyncWrite};

use crate::permissions::RandomError;
#[cfg(windows)]
use crate::permissions::{hex, random_bytes};

/// A byte stream to or from the other side.
pub(crate) trait Stream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Stream for T {}

/// The address of the app's MCP channel, as the client configuration carries it
/// (`MAIL_MCP_ENDPOINT`): a socket path on macOS, a pipe name on Windows.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Endpoint(String);

/// Longest socket path: `sun_path` holds 104 bytes on macOS, including the terminating NUL.
#[cfg(unix)]
const MAX_SOCKET_PATH: usize = 103;

#[cfg(windows)]
const PIPE_PREFIX: &str = r"\\.\pipe\";

impl Endpoint {
    /// Checks the form: an absolute socket path of at most 103 bytes (macOS), or
    /// `\\.\pipe\<name>` (Windows).
    pub fn new(address: impl Into<String>) -> Result<Self, EndpointError> {
        let address = address.into();
        #[cfg(unix)]
        {
            if !Path::new(&address).is_absolute() || address.contains('\0') {
                return Err(EndpointError::Invalid(address));
            }
            if address.len() > MAX_SOCKET_PATH {
                return Err(EndpointError::TooLong(address));
            }
        }
        #[cfg(windows)]
        {
            let name = address.strip_prefix(PIPE_PREFIX).unwrap_or_default();
            if name.is_empty() || name.contains(['\\', '/', '\0']) || address.len() > 256 {
                return Err(EndpointError::Invalid(address));
            }
        }
        Ok(Self(address))
    }

    /// The endpoint for a data directory. macOS: `<data dir>/mcp/mail-mcp.sock`. Windows: a
    /// new random pipe name; the app stores it and uses it on every start (the client
    /// configuration contains it).
    pub fn for_data_dir(data_dir: &Path) -> Result<Self, EndpointError> {
        #[cfg(unix)]
        {
            let path = data_dir.join("mcp").join("mail-mcp.sock");
            Self::new(path.to_string_lossy().into_owned())
        }
        #[cfg(windows)]
        {
            let _ = data_dir;
            Self::random_pipe()
        }
    }

    /// `\\.\pipe\mail-mcp-<32 hex digits>`.
    #[cfg(windows)]
    pub fn random_pipe() -> Result<Self, EndpointError> {
        let suffix = hex(&random_bytes::<16>()?);
        Self::new(format!("{PIPE_PREFIX}mail-mcp-{suffix}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EndpointError {
    #[error("not a valid MCP endpoint: {0}")]
    Invalid(String),
    #[error("the socket path is longer than 103 bytes: {0}")]
    TooLong(String),
    #[error(transparent)]
    Random(#[from] RandomError),
}

/// Why the host could not listen.
#[derive(Debug, thiserror::Error)]
pub enum ListenError {
    #[error("another process is already listening on {0}")]
    InUse(String),
    #[error("{0} exists and is not a socket; it is left alone")]
    NotASocket(String),
    #[error("the directory of {0} belongs to another user")]
    ForeignDirectory(String),
    #[error("could not listen on {endpoint}: {source}")]
    Io {
        endpoint: String,
        #[source]
        source: io::Error,
    },
}

/// Connects to the app (helper side). `NotFound`/`ConnectionRefused` mean: nothing listens.
pub(crate) async fn connect(endpoint: &Endpoint) -> io::Result<Box<dyn Stream>> {
    #[cfg(unix)]
    {
        let stream = tokio::net::UnixStream::connect(endpoint.as_str()).await?;
        Ok(Box::new(stream))
    }
    #[cfg(windows)]
    {
        use tokio::net::windows::named_pipe::ClientOptions;
        /// `ERROR_PIPE_BUSY`: every instance is in use; another one is created right away.
        const PIPE_BUSY: i32 = 231;
        let mut attempts = 0;
        loop {
            match ClientOptions::new().open(endpoint.as_str()) {
                Ok(client) => return Ok(Box::new(client)),
                Err(err) if err.raw_os_error() == Some(PIPE_BUSY) && attempts < 40 => {
                    attempts += 1;
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
                Err(err) => return Err(err),
            }
        }
    }
}

/// The app's listening side.
pub(crate) struct Listener {
    #[cfg(unix)]
    inner: unix::UnixListenerGuard,
    #[cfg(windows)]
    inner: windows::PipeListener,
}

impl Listener {
    pub async fn bind(endpoint: &Endpoint) -> Result<Self, ListenError> {
        #[cfg(unix)]
        {
            Ok(Self {
                inner: unix::UnixListenerGuard::bind(endpoint).await?,
            })
        }
        #[cfg(windows)]
        {
            Ok(Self {
                inner: windows::PipeListener::bind(endpoint)?,
            })
        }
    }

    pub async fn accept(&mut self) -> io::Result<Box<dyn Stream>> {
        self.inner.accept().await
    }
}

#[cfg(unix)]
mod unix {
    use std::fs::{self, DirBuilder, Permissions};
    use std::io;
    use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt};
    use std::path::PathBuf;

    use tokio::net::{UnixListener, UnixStream};

    use super::{Endpoint, ListenError, Stream};

    /// The listener; removes its socket file when dropped (if it is still the same file).
    pub(super) struct UnixListenerGuard {
        listener: UnixListener,
        path: PathBuf,
        /// Device and inode of the socket file, to remove only our own.
        file: (u64, u64),
        uid: u32,
    }

    impl UnixListenerGuard {
        pub async fn bind(endpoint: &Endpoint) -> Result<Self, ListenError> {
            let path = PathBuf::from(endpoint.as_str());
            let io_error = |source| ListenError::Io {
                endpoint: endpoint.as_str().to_owned(),
                source,
            };
            let dir = path
                .parent()
                .ok_or_else(|| io_error(io::Error::other("the socket path has no directory")))?;
            DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(dir)
                .map_err(io_error)?;
            fs::set_permissions(dir, Permissions::from_mode(0o700)).map_err(io_error)?;
            match fs::symlink_metadata(&path) {
                Ok(meta) if meta.file_type().is_socket() => {
                    if UnixStream::connect(&path).await.is_ok() {
                        return Err(ListenError::InUse(endpoint.as_str().to_owned()));
                    }
                    // Left over from a run that did not end cleanly.
                    fs::remove_file(&path).map_err(io_error)?;
                }
                Ok(_) => return Err(ListenError::NotASocket(endpoint.as_str().to_owned())),
                Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                Err(err) => return Err(io_error(err)),
            }
            let listener = UnixListener::bind(&path).map_err(io_error)?;
            fs::set_permissions(&path, Permissions::from_mode(0o600)).map_err(io_error)?;
            let meta = fs::symlink_metadata(&path).map_err(io_error)?;
            let dir_meta = fs::metadata(dir).map_err(io_error)?;
            // The socket was just created by this process, so its owner is us.
            if dir_meta.uid() != meta.uid() {
                drop(listener);
                let _ = fs::remove_file(&path);
                return Err(ListenError::ForeignDirectory(endpoint.as_str().to_owned()));
            }
            Ok(Self {
                listener,
                file: (meta.dev(), meta.ino()),
                uid: meta.uid(),
                path,
            })
        }

        pub async fn accept(&mut self) -> io::Result<Box<dyn Stream>> {
            loop {
                let (stream, _) = self.listener.accept().await?;
                match stream.peer_cred() {
                    Ok(cred) if cred.uid() == self.uid => return Ok(Box::new(stream)),
                    Ok(cred) => {
                        tracing::warn!(
                            uid = cred.uid(),
                            "MCP connection from another user refused"
                        );
                    }
                    Err(err) => tracing::warn!(error = %err, "MCP peer credentials unavailable"),
                }
            }
        }
    }

    impl Drop for UnixListenerGuard {
        fn drop(&mut self) {
            if let Ok(meta) = fs::symlink_metadata(&self.path)
                && (meta.dev(), meta.ino()) == self.file
            {
                let _ = fs::remove_file(&self.path);
            }
        }
    }
}

#[cfg(windows)]
mod windows {
    use std::io;

    use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};

    use super::{Endpoint, ListenError, Stream};

    /// `ERROR_ACCESS_DENIED`: with `first_pipe_instance`, a pipe of this name exists already.
    const ACCESS_DENIED: i32 = 5;

    pub(super) struct PipeListener {
        name: String,
        next: NamedPipeServer,
    }

    impl PipeListener {
        pub fn bind(endpoint: &Endpoint) -> Result<Self, ListenError> {
            let next = ServerOptions::new()
                .first_pipe_instance(true)
                .reject_remote_clients(true)
                .create(endpoint.as_str())
                .map_err(|source| {
                    if source.raw_os_error() == Some(ACCESS_DENIED) {
                        ListenError::InUse(endpoint.as_str().to_owned())
                    } else {
                        ListenError::Io {
                            endpoint: endpoint.as_str().to_owned(),
                            source,
                        }
                    }
                })?;
            Ok(Self {
                name: endpoint.as_str().to_owned(),
                next,
            })
        }

        pub async fn accept(&mut self) -> io::Result<Box<dyn Stream>> {
            self.next.connect().await?;
            let fresh = ServerOptions::new()
                .reject_remote_clients(true)
                .create(&self.name)?;
            let connected = std::mem::replace(&mut self.next, fresh);
            Ok(Box::new(connected))
        }
    }
}
