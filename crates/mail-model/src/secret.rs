use std::fmt;

/// Text shown instead of a secret value in `Debug` and `Display` output.
pub const REDACTED: &str = "[redacted]";

/// Wraps a password, token or API key so it cannot end up in logs or error messages by accident.
///
/// - `Debug` and `Display` print [`REDACTED`], never the value. This covers `{:?}`, `{}`,
///   `tracing` fields (`?secret`, `%secret`) and any `#[derive(Debug)]` struct holding a secret.
/// - There is deliberately no `Serialize` impl, so a secret cannot cross the IPC bridge or be
///   written to the database through serde.
/// - The value is only reachable through [`Secret::expose_secret`], which makes every use
///   explicit and easy to find in review.
#[derive(Clone, Default)]
pub struct Secret<T>(T);

impl<T> Secret<T> {
    pub const fn new(value: T) -> Self {
        Self(value)
    }

    /// Returns the wrapped value. Only call this where the value is handed to its consumer
    /// (keychain, server login); never format or log the result.
    pub const fn expose_secret(&self) -> &T {
        &self.0
    }

    /// Unwraps the value, see [`Secret::expose_secret`].
    pub fn into_exposed(self) -> T {
        self.0
    }
}

impl<T> From<T> for Secret<T> {
    fn from(value: T) -> Self {
        Self(value)
    }
}

impl<T> fmt::Debug for Secret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

impl<T> fmt::Display for Secret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CoreError, IpcError};

    const PLAIN: &str = "hunter2-very-secret";

    #[derive(Debug)]
    #[allow(dead_code)] // fields are only read through `Debug`
    struct Login {
        user: String,
        password: Secret<String>,
    }

    #[test]
    fn debug_and_display_are_redacted() {
        let secret = Secret::new(PLAIN.to_string());
        assert_eq!(format!("{secret:?}"), REDACTED);
        assert_eq!(format!("{secret}"), REDACTED);
        assert_eq!(format!("{secret:#?}"), REDACTED);

        let login = Login {
            user: "alice".into(),
            password: secret.clone(),
        };
        let dbg = format!("{login:?} {login:#?}");
        assert!(!dbg.contains(PLAIN), "{dbg}");
        assert!(dbg.contains(REDACTED));
    }

    #[test]
    fn value_is_reachable_only_explicitly() {
        let secret = Secret::new(PLAIN.to_string());
        assert_eq!(secret.expose_secret(), PLAIN);
        assert_eq!(secret.into_exposed(), PLAIN);
    }

    /// A secret that is formatted into an error (the usual accident) stays redacted in the
    /// error's `Display`, its `Debug` and the IPC form that reaches the UI.
    #[test]
    fn secret_never_appears_in_error_messages() {
        let secret = Secret::new(PLAIN.to_string());
        let err = CoreError::Internal(format!("login rejected for password {secret}"));
        let ipc = IpcError::from(&err);
        let wire = serde_json::to_string(&ipc).expect("serialize IpcError");

        for text in [
            err.to_string(),
            format!("{err:?}"),
            format!("{ipc:?}"),
            wire,
            err.detail().unwrap_or_default(),
        ] {
            assert!(!text.contains(PLAIN), "secret leaked: {text}");
        }
        assert!(err.to_string().contains(REDACTED));
    }
}
