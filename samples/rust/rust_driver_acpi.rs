// SPDX-License-Identifier: GPL-2.0

//! Rust ACPI driver sample.

use kernel::{acpi, c_str, device, prelude::*, types::ARef};

struct SampleDriver {
    adev: ARef<acpi::Device>,
}

kernel::acpi_device_table!(
    ACPI_TABLE,
    MODULE_ACPI_TABLE,
    <SampleDriver as acpi::Driver>::IdInfo,
    [(acpi::DeviceId::new(c_str!("PNP0C0C")), ())]
);

impl acpi::Driver for SampleDriver {
    type IdInfo = ();
    const ACPI_ID_TABLE: Option<acpi::IdTable<Self::IdInfo>> = Some(&ACPI_TABLE);
    const CLASS: Option<&'static CStr> = Some(c_str!("rust-acpi"));

    fn add(
        adev: &acpi::Device<device::Core>,
        _info: Option<&Self::IdInfo>,
    ) -> Result<Pin<KBox<Self>>> {
        dev_dbg!(adev.as_ref(), "Add Rust ACPI driver sample.\n");

        let drvdata = KBox::new(Self { adev: adev.into() }, GFP_KERNEL)?;

        Ok(drvdata.into())
    }

    fn notify(adev: &acpi::Device<device::Core>, event: u32) {
        dev_info!(adev.as_ref(), "Receive ACPI event '{event}'.\n");
    }
}

impl Drop for SampleDriver {
    fn drop(&mut self) {
        dev_dbg!(self.adev.as_ref(), "Remove Rust ACPI driver sample.\n");
    }
}

kernel::module_acpi_driver! {
    type: SampleDriver,
    name: "rust_driver_acpi",
    authors: ["Luo Qiu"],
    description: "Rust ACPI driver",
    license: "GPL v2",
}
