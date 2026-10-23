use std::io;

#[derive(Debug)]
pub(crate) enum Error {
    Io(io::Error),
    MailboxTimeout { queue: u8, msg: u8 },
    MailboxStatus { queue: u8, msg: u8, status: u8 },
    Parse(String),
    Page(String),
    Unlock(String),
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(e) => write!(f, "{e}"),
            Error::MailboxTimeout { queue, msg } => {
                write!(f, "queue {queue} msg 0x{msg:02X} timed out")
            }
            Error::MailboxStatus { queue, msg, status } => {
                write!(f, "queue {queue} msg 0x{msg:02X} returned 0x{status:02X}")
            }
            Error::Parse(s) => write!(f, "{s}"),
            Error::Page(s) => write!(f, "{s}"),
            Error::Unlock(s) => write!(f, "{s}"),
        }
    }
}
