// The overwhelming majority of the code in this crate is adapted from https://github.com/stephaneyfx/brightness
// This crate is a stripped down version of brightness, which removes all display brightness-related
// functionality, and all Linux-focused functionality, while retaining (and slightly modifying) the
// "blocking" Windows code to retrieve detailed monitor display data for use in https://github.com/LGUG2Z/komorebi

mod device;
pub mod error;

pub use device::Device;
pub use error::Error;

pub fn connected_displays_physical(
) -> impl Iterator<Item = Result<device::PhysicalDevice, error::Error>> {
    device::connected_displays_physical().map(|r| r.map_err(Into::into))
}

pub fn connected_displays_all() -> impl Iterator<Item = Result<device::Device, error::Error>> {
    device::connected_displays_all().map(|r| r.map_err(Into::into))
}
