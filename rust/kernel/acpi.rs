// SPDX-License-Identifier: GPL-2.0

//! Abstractions for the ACPI bus.
//!
//! C header: [`include/acpi/acpi_bus.h`](srctree/include/acpi/acpi_bus.h)

use crate::{
    bindings, device,
    device_id::RawDeviceId,
    driver,
    error::{to_result, Result},
    prelude::*,
    types::{ARef, ForeignOwnable, Opaque},
};
use core::{
    marker::PhantomData,
    ops::Deref,
    ptr::{addr_of_mut, NonNull},
};

/// An adapter for the registration of ACPI drivers.
pub struct Adapter<T: Driver>(T);

// SAFETY: A call to `unregister` for a given instance of `RegType` is guaranteed to be valid if
// a preceding call to `register` has been successful.
unsafe impl<T: Driver + 'static> driver::RegistrationOps for Adapter<T> {
    type RegType = bindings::acpi_driver;

    unsafe fn register(
        adrv: &Opaque<Self::RegType>,
        name: &'static CStr,
        module: &'static ThisModule,
    ) -> Result {
        const MAX_LEN: usize = 80;

        let acpi_table = match T::ACPI_ID_TABLE {
            Some(table) => table.as_ptr(),
            None => core::ptr::null(),
        };

        // SAFETY: It's safe to set the fields of `struct acpi_driver` on initialization.
        unsafe {
            (*adrv.get()).ids = acpi_table;
            (*adrv.get()).ops.add = Some(Self::add_callback);
            (*adrv.get()).ops.remove = Some(Self::remove_callback);
            (*adrv.get()).ops.notify = Some(Self::notify_callback);

            let src = name.as_bytes_with_nul();
            let len = src.len().min(MAX_LEN);
            let range = 0..len;
            (*adrv.get()).name[range.clone()].clone_from_slice(&src[range]);

            if let Some(class) = T::CLASS {
                let src = class.as_bytes_with_nul();
                let len = src.len().min(MAX_LEN);
                let range = 0..len;
                (*adrv.get()).class[range.clone()].clone_from_slice(&src[range]);
            }
        }

        // SAFETY: `adrv` is guaranteed to be a valid `RegType`.
        to_result(unsafe { bindings::__acpi_bus_register_driver(adrv.get(), module.0) })
    }

    unsafe fn unregister(adrv: &Opaque<Self::RegType>) {
        // SAFETY: `adrv` is guaranteed to be a valid `RegType`.
        unsafe { bindings::acpi_bus_unregister_driver(adrv.get()) }
    }
}

impl<T: Driver + 'static> Adapter<T> {
    extern "C" fn add_callback(adev: *mut bindings::acpi_device) -> kernel::ffi::c_int {
        // SAFETY: The ACPI bus only ever calls the add callback with a valid pointer to a
        // `struct acpi_device`.
        //
        // INVARIANT: `adev` is valid for the duration of `add_callback()`.
        let adev = unsafe { &*adev.cast::<Device<device::Core>>() };

        let info = <Self as driver::Adapter>::id_info(adev.as_ref());
        match T::add(adev, info) {
            Ok(data) => unsafe {
                // Let the `struct acpi_device` own a reference of the driver's private data.
                // SAFETY: By the type invariant `adev.as_raw` returns a valid pointer to a
                // `struct acpi_device`.
                bindings::acpi_set_drvdata(adev.as_raw(), data.into_foreign() as _)
            },

            Err(err) => return Error::to_errno(err),
        }

        0
    }

    extern "C" fn remove_callback(adev: *mut bindings::acpi_device) {
        // SAFETY: The ACPI bus only ever calls the remove callback with a valid pointer to a
        // `struct acpi_device`.
        let ptr = unsafe { bindings::acpi_get_drvdata(adev) };

        // SAFETY: `remove_callback` is only ever called after a successful call to
        // `add_callback`, hence it's guaranteed that `ptr` points to a valid and initialized
        // `KBox<T>` pointer created through `KBox::into_foreign`.
        let _ = unsafe { KBox::<T>::from_foreign(ptr) };
    }

    extern "C" fn notify_callback(adev: *mut bindings::acpi_device, event: u32) {
        // SAFETY: The ACPI bus only ever calls the notify callback with a valid pointer to a
        // `struct acpi_device`.
        //
        // INVARIANT: `adev` is valid for the duration of `add_callback()`.
        let adev = unsafe { &*adev.cast::<Device<device::Core>>() };

        T::notify(adev, event)
    }
}

impl<T: Driver + 'static> driver::Adapter for Adapter<T> {
    type IdInfo = T::IdInfo;

    fn of_id_table() -> Option<crate::of::IdTable<Self::IdInfo>> {
        None
    }

    fn acpi_id_table() -> Option<self::IdTable<Self::IdInfo>> {
        T::ACPI_ID_TABLE
    }
}

/// Declares a kernel module that exposes a single ACPI driver.
///
/// # Example
///
///```ignore
/// kernel::module_acpi_driver! {
///     type: MyDriver,
///     name: "Module name",
///     authors: ["Author name"],
///     description: "Description",
///     license: "GPL v2",
/// }
///```
#[macro_export]
macro_rules! module_acpi_driver {
    ($($f:tt)*) => {
        $crate::module_driver!(<T>, $crate::acpi::Adapter<T>, { $($f)* });
    };
}

/// Abstraction for `bindings::acpi_device_id`.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct DeviceId(bindings::acpi_device_id);

impl DeviceId {
    /// Create a new `acpi::DeviceId` from an ACPI ID string.
    pub const fn new(id: &'static CStr) -> Self {
        let src = id.as_bytes_with_nul();
        // Replace with `bindings::acpi_device_id::default()` once stabilized for `const`.
        // SAFETY: FFI type is valid to be zero-initialized.
        let mut acpi: bindings::acpi_device_id = unsafe { core::mem::zeroed() };

        // TODO: Use `clone_from_slice` once the corresponding types do match.
        let mut i = 0;
        while i < src.len() {
            acpi.id[i] = src[i] as _;
            i += 1;
        }

        Self(acpi)
    }

    /// Equivalent to C's `ACPI_DEVICE_CLASS` macro.
    ///
    /// Create a new `acpi::DeviceId` from a class number and mask.
    pub const fn from_class(class: u32, class_mask: u32) -> Self {
        // Replace with `bindings::acpi_device_id::default()` once stabilized for `const`.
        // SAFETY: FFI type is valid to be zero-initialized.
        let mut acpi: bindings::acpi_device_id = unsafe { core::mem::zeroed() };

        acpi.cls = class;
        acpi.cls_msk = class_mask;

        Self(acpi)
    }
}

// SAFETY:
// * `DeviceId` is a `#[repr(transparent)` wrapper of `struct acpi_device_id` and does not add
//   additional invariants, so it's safe to transmute to `RawType`.
// * `DRIVER_DATA_OFFSET` is the offset to the `driver_data` field.
unsafe impl RawDeviceId for DeviceId {
    type RawType = bindings::acpi_device_id;

    const DRIVER_DATA_OFFSET: usize = core::mem::offset_of!(bindings::acpi_device_id, driver_data);

    fn index(&self) -> usize {
        self.0.driver_data as _
    }
}

/// IdTable type for ACPI drivers.
pub type IdTable<T> = &'static dyn kernel::device_id::IdTable<DeviceId, T>;

/// Create an ACPI `IdTable` with an "alias" for modpost.
#[macro_export]
macro_rules! acpi_device_table {
    ($table_name:ident, $module_table_name:ident, $id_info_type: ty, $table_data: expr) => {
        const $table_name: $crate::device_id::IdArray<
            $crate::acpi::DeviceId,
            $id_info_type,
            { $table_data.len() },
        > = $crate::device_id::IdArray::new($table_data);

        $crate::module_device_table!("acpi", $module_table_name, $table_name);
    };
}

/// The ACPI driver trait.
///
/// # Example
///
///```
/// # use kernel::{acpi, c_str, device, prelude::*};
///
/// struct MyDriver;
///
/// kernel::acpi_device_table!(
///     ACPI_TABLE,
///     MODULE_ACPI_TABLE,
///     <MyDriver as acpi::Driver>::IdInfo,
///     [
///         (acpi::DeviceId::new(c_str!("ABCD1234")), ())
///     ]
/// );
///
/// impl acpi::Driver for MyDriver {
///     type IdInfo = ();
///     const ACPI_ID_TABLE: Option<acpi::IdTable<Self::IdInfo>> = Some(&ACPI_TABLE);
///
///     fn add(
///         _dev: &acpi::Device<device::Core>,
///         _id_info: Option<&Self::IdInfo>,
///     ) -> Result<Pin<KBox<Self>>> {
///         Err(ENODEV)
///     }
///
///     fn notify(_dev: &acpi::Device<device::Core>, _event: u32) {}
/// }
///```
pub trait Driver: Send {
    /// The type holding information about each device id supported by the driver.
    ///
    /// TODO: Use associated_type_defaults once stabilized:
    ///
    /// type IdInfo: 'static = ();
    type IdInfo: 'static;

    /// The table of device ids supported by the driver.
    const ACPI_ID_TABLE: Option<IdTable<Self::IdInfo>>;

    /// The class of ACPI driver.
    const CLASS: Option<&'static CStr> = None;

    /// ACPI driver add hook.
    ///
    /// Called when a new ACPI device is added or discovered.
    /// Implementers should attempt to initialize the device here.
    fn add(dev: &Device<device::Core>, id_info: Option<&Self::IdInfo>) -> Result<Pin<KBox<Self>>>;

    /// ACPI driver notify hook.
    ///
    /// Called when a new ACPI device handles notifications.
    fn notify(dev: &Device<device::Core>, event: u32);
}

/// The ACPI device representation.
///
/// This structure represents the Rust abstraction for a C `struct acpi_device`. The implementation
/// abstracts the usage of an already existing C `struct acpi_device` within Rust code that we get
/// passed from the C side.
///
/// # Invariants
///
/// A [`Device`] instance represents a valid `struct acpi_device` created by the C portion of the kernel.
#[repr(transparent)]
pub struct Device<Ctx: device::DeviceContext = device::Normal>(
    Opaque<bindings::acpi_device>,
    PhantomData<Ctx>,
);

impl Device {
    fn as_raw(&self) -> *mut bindings::acpi_device {
        self.0.get()
    }
}

impl Deref for Device<device::Core> {
    type Target = Device;

    fn deref(&self) -> &Self::Target {
        let ptr: *const Self = self;

        // CAST: `Device<Ctx>` is a transparent wrapper of `Opaque<bindings::acpi_device>`.
        let ptr = ptr.cast::<Device>();

        // SAFETY: `ptr` was derived from `&self`.
        unsafe { &*ptr }
    }
}

impl From<&Device<device::Core>> for ARef<Device> {
    fn from(dev: &Device<device::Core>) -> Self {
        (&**dev).into()
    }
}

// SAFETY: Instances of `Device` are always reference-counted.
unsafe impl crate::types::AlwaysRefCounted for Device {
    fn inc_ref(&self) {
        // SAFETY: The existence of a shared reference guarantees that the refcount is non-zero.
        unsafe { bindings::get_device(self.as_ref().as_raw()) };
    }

    unsafe fn dec_ref(obj: NonNull<Self>) {
        // SAFETY: The safety requirements guarantee that the refcount is non-zero.
        unsafe { bindings::acpi_device_put(obj.cast().as_ptr()) }
    }
}

impl AsRef<device::Device> for Device {
    fn as_ref(&self) -> &device::Device {
        // SAFETY: By the type invariant of `Self`, `self.as_raw()` is a pointer to a valid
        // `struct acpi_device`.
        let dev = unsafe { addr_of_mut!((*self.as_raw()).dev) };

        // SAFETY: `dev` points to a valid `struct device`.
        unsafe { device::Device::as_ref(dev) }
    }
}

// SAFETY: A `Device` is always reference-counted and can be released from any thread.
unsafe impl Send for Device {}

// SAFETY: `Device` can be shared among threads because all methods of `Device`
// (i.e. `Device<Normal>) are thread safe.
unsafe impl Sync for Device {}
