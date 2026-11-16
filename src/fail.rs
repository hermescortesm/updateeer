//! The few failures the app reacts to instead of only showing them. Errors travel as text (every
//! package manager words its own), so these are made and recognised here, from one exact text.
use crate::i18n::t;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Fail {
    /// The user stopped the queue and the process was killed: not a failure to report.
    Cancelled,
    /// The user dismissed the administrator password dialog: retrying would only ask again.
    PasswordCancelled,
    /// No connection at all: one short line instead of the package manager's output.
    Offline,
}

impl Fail {
    fn es(self) -> &'static str {
        match self {
            Fail::Cancelled => "Cancelado",
            Fail::PasswordCancelled => "Cancelaste la petición de contraseña",
            Fail::Offline => "Sin conexión a internet",
        }
    }

    /// What the user sees, in their language.
    pub fn text(self) -> &'static str {
        t(self.es())
    }

    /// Whether an error text is this failure.
    pub fn is(self, error: &str) -> bool {
        error == self.text()
    }
}

impl From<Fail> for String {
    fn from(f: Fail) -> String {
        f.text().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn made_and_recognised_from_the_same_text() {
        for f in [Fail::Cancelled, Fail::PasswordCancelled, Fail::Offline] {
            assert!(f.is(&String::from(f)));
        }
        assert!(!Fail::Offline.is("pacman: error: failed retrieving file"));
    }
}
