use std::collections::HashMap;
use std::ffi::OsString;
use std::iter::once;
use std::mem::size_of;
use std::os::windows::ffi::OsStringExt;
use std::ptr;

use itertools::Either;
use windows::core::PCWSTR;
use windows::Win32::Devices::Display::DestroyPhysicalMonitor;
use windows::Win32::Devices::Display::DisplayConfigGetDeviceInfo;
use windows::Win32::Devices::Display::GetDisplayConfigBufferSizes;
use windows::Win32::Devices::Display::GetNumberOfPhysicalMonitorsFromHMONITOR;
use windows::Win32::Devices::Display::GetPhysicalMonitorsFromHMONITOR;
use windows::Win32::Devices::Display::QueryDisplayConfig;
use windows::Win32::Devices::Display::DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME;
use windows::Win32::Devices::Display::DISPLAYCONFIG_MODE_INFO;
use windows::Win32::Devices::Display::DISPLAYCONFIG_MODE_INFO_TYPE_TARGET;
use windows::Win32::Devices::Display::DISPLAYCONFIG_OUTPUT_TECHNOLOGY_INTERNAL;
use windows::Win32::Devices::Display::DISPLAYCONFIG_PATH_INFO;
use windows::Win32::Devices::Display::DISPLAYCONFIG_TARGET_DEVICE_NAME;
use windows::Win32::Devices::Display::DISPLAYCONFIG_VIDEO_OUTPUT_TECHNOLOGY;
use windows::Win32::Devices::Display::PHYSICAL_MONITOR;
use windows::Win32::Devices::Display::QDC_ONLY_ACTIVE_PATHS;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::Foundation::BOOL;
use windows::Win32::Foundation::ERROR_ACCESS_DENIED;
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Foundation::LPARAM;
use windows::Win32::Foundation::RECT;
use windows::Win32::Foundation::WIN32_ERROR;
use windows::Win32::Graphics::Gdi::EnumDisplayDevicesW;
use windows::Win32::Graphics::Gdi::EnumDisplayMonitors;
use windows::Win32::Graphics::Gdi::GetMonitorInfoW;
use windows::Win32::Graphics::Gdi::DISPLAY_DEVICEW;
use windows::Win32::Graphics::Gdi::DISPLAY_DEVICE_ACTIVE;
use windows::Win32::Graphics::Gdi::HDC;
use windows::Win32::Graphics::Gdi::HMONITOR;
use windows::Win32::Graphics::Gdi::MONITORINFO;
use windows::Win32::Graphics::Gdi::MONITORINFOEXW;
use windows::Win32::Storage::FileSystem::CreateFileW;
use windows::Win32::Storage::FileSystem::FILE_GENERIC_READ;
use windows::Win32::Storage::FileSystem::FILE_GENERIC_WRITE;
use windows::Win32::Storage::FileSystem::FILE_SHARE_READ;
use windows::Win32::Storage::FileSystem::FILE_SHARE_WRITE;
use windows::Win32::Storage::FileSystem::OPEN_EXISTING;
use windows::Win32::UI::WindowsAndMessaging::EDD_GET_DEVICE_INTERFACE_NAME;

use crate::error::SysError;

#[derive(Debug)]
pub struct PhysicalDevice {
    // new stuff
    pub hmonitor: isize,
    pub size: RECT,
    pub work_area_size: RECT,
    // old stuff
    pub physical_monitor: WrappedPhysicalMonitor,
    pub file_handle: WrappedFileHandle,
    pub device_name: String,
    /// Note: PHYSICAL_MONITOR.szPhysicalMonitorDescription == DISPLAY_DEVICEW.DeviceString
    /// Description is **not** unique.
    pub device_description: String,
    pub device_key: String,
    /// Note: DISPLAYCONFIG_TARGET_DEVICE_NAME.monitorDevicePath == DISPLAY_DEVICEW.DeviceID (with EDD_GET_DEVICE_INTERFACE_NAME)\
    /// These are in the "DOS Device Path" format.
    pub device_path: String,
    pub output_technology: DISPLAYCONFIG_VIDEO_OUTPUT_TECHNOLOGY,
}

#[derive(Debug)]
pub struct Device {
    // new stuff
    pub hmonitor: isize,
    pub size: RECT,
    pub work_area_size: RECT,
    // old stuff
    pub device_name: String,
    /// Note: PHYSICAL_MONITOR.szPhysicalMonitorDescription == DISPLAY_DEVICEW.DeviceString
    /// Description is **not** unique.
    pub device_description: String,
    pub device_key: String,
    /// Note: DISPLAYCONFIG_TARGET_DEVICE_NAME.monitorDevicePath == DISPLAY_DEVICEW.DeviceID (with EDD_GET_DEVICE_INTERFACE_NAME)\
    /// These are in the "DOS Device Path" format.
    pub device_path: String,
    pub output_technology: DISPLAYCONFIG_VIDEO_OUTPUT_TECHNOLOGY,
}


impl PhysicalDevice {
    pub fn is_internal(&self) -> bool {
        self.output_technology == DISPLAYCONFIG_OUTPUT_TECHNOLOGY_INTERNAL
    }
}

/// A safe wrapper for a physical monitor handle that implements `Drop` to call `DestroyPhysicalMonitor`
pub struct WrappedPhysicalMonitor(HANDLE);

impl std::fmt::Debug for WrappedPhysicalMonitor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0 .0)
    }
}

impl Drop for WrappedPhysicalMonitor {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyPhysicalMonitor(self.0);
        }
    }
}

/// A safe wrapper for a windows HANDLE that implements `Drop` to call `CloseHandle`
pub struct WrappedFileHandle(HANDLE);

impl std::fmt::Debug for WrappedFileHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0 .0)
    }
}

impl Drop for WrappedFileHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

#[inline]
fn flag_set<T: std::ops::BitAnd<Output = T> + PartialEq + Copy>(t: T, flag: T) -> bool {
    t & flag == flag
}

pub fn connected_displays_all() -> impl Iterator<Item = Result<Device, SysError>> {
    unsafe {
        let device_info_map = match get_device_info_map() {
            Ok(info) => info,
            Err(e) => return Either::Right(once(Err(e))),
        };

        let hmonitors = match enum_display_monitors() {
            Ok(monitors) => monitors,
            Err(e) => return Either::Right(once(Err(e))),
        };

        Either::Left(hmonitors.into_iter().flat_map(move |hmonitor| {
            let display_devices = match get_display_devices_from_hmonitor(hmonitor) {
                Ok(p) => p,
                Err(e) => return vec![Err(e)],
            };

            display_devices
                .into_iter()
                .map(
                    |(monitor_info, display_device)| {
                        let info = device_info_map
                            .get(&display_device.DeviceID)
                            .ok_or(SysError::DeviceInfoMissing)?;

                        Ok(Device {
                            hmonitor: hmonitor.0,
                            size: monitor_info.monitorInfo.rcMonitor,
                            work_area_size: monitor_info.monitorInfo.rcWork,
                            device_name: wchar_to_string(&display_device.DeviceName),
                            device_description: wchar_to_string(&display_device.DeviceString),
                            device_key: wchar_to_string(&display_device.DeviceKey),
                            device_path: wchar_to_string(&display_device.DeviceID),
                            output_technology: info.outputTechnology,
                        })
                    },
                )
                .collect()
        }))
    }
}


pub fn connected_displays_physical() -> impl Iterator<Item = Result<PhysicalDevice, SysError>> {
    unsafe {
        let device_info_map = match get_device_info_map() {
            Ok(info) => info,
            Err(e) => return Either::Right(once(Err(e))),
        };

        let hmonitors = match enum_display_monitors() {
            Ok(monitors) => monitors,
            Err(e) => return Either::Right(once(Err(e))),
        };

        Either::Left(hmonitors.into_iter().flat_map(move |hmonitor| {
            let physical_monitors = match get_physical_monitors_from_hmonitor(hmonitor) {
                Ok(p) => p,
                Err(e) => return vec![Err(e)],
            };
            let display_devices = match get_display_devices_from_hmonitor(hmonitor) {
                Ok(p) => p,
                Err(e) => return vec![Err(e)],
            };
            if display_devices.len() != physical_monitors.len() {
                // There doesn't seem to be any way to directly associate a physical monitor
                // handle with the equivalent display device, other than by array indexing
                // https://stackoverflow.com/questions/63095216/how-to-associate-physical-monitor-with-monitor-deviceid
                return vec![Err(SysError::EnumerationMismatch)];
            }
            physical_monitors
                .into_iter()
                .zip(display_devices)
                .filter_map(|(physical_monitor, (monitor_info, display_device))| {
                    get_file_handle_for_display_device(&display_device)
                        .transpose()
                        .map(|file_handle| {
                            (monitor_info, physical_monitor, display_device, file_handle)
                        })
                })
                .map(
                    |(monitor_info, physical_monitor, display_device, file_handle)| {
                        let file_handle = file_handle?;
                        let info = device_info_map
                            .get(&display_device.DeviceID)
                            .ok_or(SysError::DeviceInfoMissing)?;
                        Ok(PhysicalDevice {
                            hmonitor: hmonitor.0,
                            size: monitor_info.monitorInfo.rcMonitor,
                            work_area_size: monitor_info.monitorInfo.rcWork,
                            physical_monitor,
                            file_handle,
                            device_name: wchar_to_string(&display_device.DeviceName),
                            device_description: wchar_to_string(&display_device.DeviceString),
                            device_key: wchar_to_string(&display_device.DeviceKey),
                            device_path: wchar_to_string(&display_device.DeviceID),
                            output_technology: info.outputTechnology,
                        })
                    },
                )
                .collect()
        }))
    }
}

/// Returns a `HashMap` of Device Path to `DISPLAYCONFIG_TARGET_DEVICE_NAME`.\
/// This can be used to find the `DISPLAYCONFIG_VIDEO_OUTPUT_TECHNOLOGY` for a monitor.\
/// The output technology is used to determine if a device is internal or external.
unsafe fn get_device_info_map(
) -> Result<HashMap<[u16; 128], DISPLAYCONFIG_TARGET_DEVICE_NAME>, SysError> {
    let mut path_count = 0;
    let mut mode_count = 0;
    GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut path_count, &mut mode_count)
        .ok()
        .map_err(SysError::GetDisplayConfigBufferSizesFailed)?;
    let mut display_paths = vec![DISPLAYCONFIG_PATH_INFO::default(); path_count as usize];
    let mut display_modes = vec![DISPLAYCONFIG_MODE_INFO::default(); mode_count as usize];
    QueryDisplayConfig(
        QDC_ONLY_ACTIVE_PATHS,
        &mut path_count,
        display_paths.as_mut_ptr(),
        &mut mode_count,
        display_modes.as_mut_ptr(),
        Some(std::ptr::null_mut()),
    )
    .ok()
    .map_err(SysError::QueryDisplayConfigFailed)?;

    display_modes
        .into_iter()
        .filter(|mode| mode.infoType == DISPLAYCONFIG_MODE_INFO_TYPE_TARGET)
        .flat_map(|mode| {
            let mut device_name = DISPLAYCONFIG_TARGET_DEVICE_NAME::default();
            device_name.header.size = size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32;
            device_name.header.adapterId = mode.adapterId;
            device_name.header.id = mode.id;
            device_name.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME;

            match WIN32_ERROR(DisplayConfigGetDeviceInfo(&mut device_name.header) as u32) {
                ERROR_SUCCESS => Some(Ok((device_name.monitorDevicePath, device_name))),
                // This error occurs if the calling process does not have access to the current desktop or is running on a remote session.
                ERROR_ACCESS_DENIED => None,
                _ => Some(Err(SysError::DisplayConfigGetDeviceInfoFailed(
                    WIN32_ERROR(DisplayConfigGetDeviceInfo(&mut device_name.header) as u32).into(),
                ))),
            }
        })
        .collect()
}

/// Calls `EnumDisplayMonitors` and returns a list of `HMONITOR` handles.\
/// Note that a `HMONITOR` is a logical construct that may correspond to multiple physical monitors.\
/// e.g. when in "Duplicate" mode two physical monitors will belong to the same `HMONITOR`
unsafe fn enum_display_monitors() -> Result<Vec<HMONITOR>, SysError> {
    unsafe extern "system" fn enum_monitors(
        handle: HMONITOR,
        _: HDC,
        _: *mut RECT,
        data: LPARAM,
    ) -> BOOL {
        let monitors = &mut *(data.0 as *mut Vec<HMONITOR>);
        monitors.push(handle);
        true.into()
    }
    let mut hmonitors = Vec::<HMONITOR>::new();
    EnumDisplayMonitors(
        HDC::default(),
        Some(ptr::null_mut()),
        Some(enum_monitors),
        LPARAM(&mut hmonitors as *mut _ as isize),
    )
    .ok()
    .map_err(SysError::EnumDisplayMonitorsFailed)?;
    Ok(hmonitors)
}

/// Gets the list of `PHYSICAL_MONITOR` handles that belong to a `HMONITOR`.\
/// These handles are required for use with the DDC/CI functions, however a valid handle will still
/// be returned for non DDC/CI monitors and also Remote Desktop Session displays.\
/// Also note that physically connected but disabled (inactive) monitors are not returned from this API.
unsafe fn get_physical_monitors_from_hmonitor(
    hmonitor: HMONITOR,
) -> Result<Vec<WrappedPhysicalMonitor>, SysError> {
    let mut physical_number: u32 = 0;
    GetNumberOfPhysicalMonitorsFromHMONITOR(hmonitor, &mut physical_number)
        .map_err(SysError::GetPhysicalMonitorsFailed)?;
    let mut raw_physical_monitors = vec![PHYSICAL_MONITOR::default(); physical_number as usize];
    // Allocate first so that pushing the wrapped handles always succeeds.
    let mut physical_monitors = Vec::with_capacity(raw_physical_monitors.len());
    GetPhysicalMonitorsFromHMONITOR(hmonitor, &mut raw_physical_monitors)
        .map_err(SysError::GetPhysicalMonitorsFailed)?;
    // Transform immediately into WrappedPhysicalMonitor so the handles don't leak
    raw_physical_monitors
        .into_iter()
        .for_each(|pm| physical_monitors.push(WrappedPhysicalMonitor(pm.hPhysicalMonitor)));
    Ok(physical_monitors)
}

/// Gets the list of display devices that belong to a `HMONITOR`.\
/// Due to the `EDD_GET_DEVICE_INTERFACE_NAME` flag, the `DISPLAY_DEVICEW` will contain the DOS
/// device path for each monitor in the `DeviceID` field.\
/// Note: Connected but inactive displays have been filtered out.
unsafe fn get_display_devices_from_hmonitor(
    hmonitor: HMONITOR,
) -> Result<Vec<(MONITORINFOEXW, DISPLAY_DEVICEW)>, SysError> {
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
    let info_ptr = &mut info as *mut _ as *mut MONITORINFO;
    GetMonitorInfoW(hmonitor, info_ptr)
        .ok()
        .map_err(SysError::GetMonitorInfoFailed)?;
    Ok((0..)
        .map_while(|device_number| {
            let mut device = DISPLAY_DEVICEW {
                cb: size_of::<DISPLAY_DEVICEW>() as u32,
                ..Default::default()
            };
            EnumDisplayDevicesW(
                PCWSTR(info.szDevice.as_ptr()),
                device_number,
                &mut device,
                EDD_GET_DEVICE_INTERFACE_NAME,
            )
            .as_bool()
            .then_some(device)
        })
        .filter(|device| flag_set(device.StateFlags, DISPLAY_DEVICE_ACTIVE))
        .map(|device| (info, device))
        .collect())
}

/// Opens and returns a file handle for a display device using its DOS device path.\
/// These handles are only used for the `DeviceIoControl` API (for internal displays); a
/// handle can still be returned for external displays, but it should not be used.\
/// A `None` value means that a handle could not be opened, but this was for an expected reason,
/// indicating this display device should be skipped.
unsafe fn get_file_handle_for_display_device(
    display_device: &DISPLAY_DEVICEW,
) -> Result<Option<WrappedFileHandle>, SysError> {
    CreateFileW(
        PCWSTR(display_device.DeviceID.as_ptr()),
        FILE_GENERIC_READ.0 | FILE_GENERIC_WRITE.0,
        FILE_SHARE_READ | FILE_SHARE_WRITE,
        Some(ptr::null_mut()),
        OPEN_EXISTING,
        Default::default(),
        HANDLE::default(),
    )
    .map(|h| Some(WrappedFileHandle(h)))
    .or_else(|e| {
        // This error occurs for virtual devices e.g. Remote Desktop
        // sessions - they are not real monitors
        (e.code() == ERROR_ACCESS_DENIED.to_hresult())
            .then_some(None)
            .ok_or_else(|| SysError::OpeningMonitorDeviceInterfaceHandleFailed {
                device_name: wchar_to_string(&display_device.DeviceName),
                source: e,
            })
    })
}

fn wchar_to_string(s: &[u16]) -> String {
    let end = s.iter().position(|&x| x == 0).unwrap_or(s.len());
    let truncated = &s[0..end];
    OsString::from_wide(truncated).to_string_lossy().into()
}
