#![warn(clippy::all, clippy::nursery, clippy::pedantic)]

use std::error::Error as StdError;

use thiserror::Error;
use windows::core::Error as WinError;

/// Errors used in this API
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    /// Getting a list of brightness devices failed
    #[error("Failed to list brightness devices")]
    ListingDevicesFailed(#[source] Box<dyn StdError + Send + Sync>),
}

#[derive(Clone, Debug, Error)]
pub(crate) enum SysError {
    #[error("Failed to enumerate device monitors")]
    EnumDisplayMonitorsFailed(#[source] WinError),
    #[error("Failed to get display config buffer sizes")]
    GetDisplayConfigBufferSizesFailed(#[source] WinError),
    #[error("Failed to query display config")]
    QueryDisplayConfigFailed(#[source] WinError),
    #[error("Failed to get display config device info")]
    DisplayConfigGetDeviceInfoFailed(#[source] WinError),
    #[error("Failed to get monitor info")]
    GetMonitorInfoFailed(#[source] WinError),
    #[error("Failed to get physical monitors from the HMONITOR")]
    GetPhysicalMonitorsFailed(#[source] WinError),
    #[error(
    "The length of GetPhysicalMonitorsFromHMONITOR() and EnumDisplayDevicesW() results did not \
     match, this could be because monitors were connected/disconnected while loading devices"
    )]
    EnumerationMismatch,
    #[error(
    "Unable to find a matching device info for this display device, this could be because monitors \
     were connected while loading devices"
    )]
    DeviceInfoMissing,
    #[error("Failed to open monitor interface handle (CreateFileW)")]
    OpeningMonitorDeviceInterfaceHandleFailed {
        device_name: String,
        source: WinError,
    },
}

impl From<SysError> for Error {
    fn from(e: SysError) -> Self {
        match &e {
            SysError::EnumerationMismatch
            | SysError::DeviceInfoMissing
            | SysError::GetDisplayConfigBufferSizesFailed(..)
            | SysError::QueryDisplayConfigFailed(..)
            | SysError::DisplayConfigGetDeviceInfoFailed(..)
            | SysError::GetPhysicalMonitorsFailed(..)
            | SysError::EnumDisplayMonitorsFailed(..)
            | SysError::GetMonitorInfoFailed(..)
            | SysError::OpeningMonitorDeviceInterfaceHandleFailed { .. } => {
                Self::ListingDevicesFailed(Box::new(e))
            }
        }
    }
}
