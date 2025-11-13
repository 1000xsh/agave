//! Error types for CPU affinity operations.

use std::io;
use thiserror::Error;

/// Errors that can occur during CPU affinity operations.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum CpuAffinityError {
    /// I/O or system call error
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    /// Operation not supported on this platform
    #[error("CPU affinity operations are not supported on this platform")]
    NotSupported,

    /// Invalid CPU ID
    #[error("CPU {cpu} is invalid (max CPU is {max})")]
    InvalidCpu { cpu: usize, max: usize },

    /// Invalid physical core ID
    #[error("Physical core {core} is invalid (max core is {max})")]
    InvalidPhysicalCore { core: usize, max: usize },

    /// CPU list is empty
    #[error("CPU list cannot be empty")]
    EmptyCpuList,

    /// Failed to parse CPU range or ID
    #[error("Failed to parse CPU specification: {0}")]
    ParseError(String),
}

// PartialEq for testing
impl PartialEq for CpuAffinityError {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Io(a), Self::Io(b)) => {
                a.kind() == b.kind() && a.to_string() == b.to_string()
            }
            (Self::NotSupported, Self::NotSupported) => true,
            (Self::InvalidCpu { cpu: a1, max: a2 }, Self::InvalidCpu { cpu: b1, max: b2 }) => {
                a1 == b1 && a2 == b2
            }
            (
                Self::InvalidPhysicalCore { core: a1, max: a2 },
                Self::InvalidPhysicalCore { core: b1, max: b2 },
            ) => a1 == b1 && a2 == b2,
            (Self::EmptyCpuList, Self::EmptyCpuList) => true,
            (Self::ParseError(a), Self::ParseError(b)) => a == b,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = CpuAffinityError::InvalidCpu { cpu: 10, max: 7 };
        assert_eq!(err.to_string(), "CPU 10 is invalid (max CPU is 7)");

        let err = CpuAffinityError::InvalidPhysicalCore { core: 5, max: 3 };
        assert_eq!(
            err.to_string(),
            "Physical core 5 is invalid (max core is 3)"
        );

        let err = CpuAffinityError::EmptyCpuList;
        assert_eq!(err.to_string(), "CPU list cannot be empty");

        let err = CpuAffinityError::NotSupported;
        assert_eq!(
            err.to_string(),
            "CPU affinity operations are not supported on this platform"
        );

        let err = CpuAffinityError::ParseError("bad input".to_string());
        assert_eq!(err.to_string(), "Failed to parse CPU specification: bad input");
    }

    #[test]
    fn test_from_io_error() {
        let io_err = io::Error::new(io::ErrorKind::PermissionDenied, "Permission denied");
        let cpu_err: CpuAffinityError = io_err.into();
        match cpu_err {
            CpuAffinityError::Io(err) => {
                assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
                assert!(err.to_string().contains("Permission denied"));
            }
            _ => panic!("Expected Io error"),
        }
    }

    #[test]
    fn test_error_equality() {
        let err1 = CpuAffinityError::InvalidCpu { cpu: 10, max: 7 };
        let err2 = CpuAffinityError::InvalidCpu { cpu: 10, max: 7 };
        assert_eq!(err1, err2);

        let err3 = CpuAffinityError::InvalidCpu { cpu: 5, max: 7 };
        assert_ne!(err1, err3);
    }
}
