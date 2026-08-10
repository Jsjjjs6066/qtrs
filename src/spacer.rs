//! Layout spacer for adding blank space between widgets.
//!
//! Wraps [`QSpacerItem`](https://doc.qt.io/qt-6/qspaceritem.html).

use crate::ffi;
use crate::layout::{VBoxLayout, HBoxLayout};

/// Size policy constants for [`Spacer`].
pub const FIXED: i32 = 0;
pub const MINIMUM: i32 = 1;
pub const MAXIMUM: i32 = 4;
pub const PREFERRED: i32 = 5;
pub const EXPANDING: i32 = 7;
pub const MINIMUM_EXPANDING: i32 = 3;
pub const IGNORED: i32 = 11;

/// A layout spacer that pushes widgets apart.
///
/// `Spacer` is not a widget — it is a layout item that provides
/// fixed or expanding blank space in box layouts.
///
/// # Example
///
/// ```no_run
/// # use qtrs::prelude::*;
/// # use qtrs::SpacerExt;
/// let mut layout = VBoxLayout::new();
/// layout.add(PushButton::new("Top").build());
/// layout.add_spacer(Spacer::vertical(20)); // 20px gap
/// layout.add(PushButton::new("Bottom").build());
/// ```
pub struct Spacer {
    ptr: *mut ffi::QSpacerItem,
}

impl Spacer {
    /// Create a spacer with explicit width, height, and size policies.
    pub fn new(w: i32, h: i32, h_policy: i32, v_policy: i32) -> Self {
        let ptr = unsafe { ffi::QSpacerItem_new(w, h, h_policy, v_policy) };
        assert!(!ptr.is_null(), "QSpacerItem_new returned null");
        Self { ptr }
    }

    /// Create a fixed-width horizontal spacer.
    pub fn horizontal(width: i32) -> Self {
        Self::new(width, 0, FIXED, MINIMUM)
    }

    /// Create a fixed-height vertical spacer.
    pub fn vertical(height: i32) -> Self {
        Self::new(0, height, MINIMUM, FIXED)
    }

    /// Create an expanding horizontal spacer (pushes widgets apart).
    pub fn horizontal_expanding() -> Self {
        Self::new(0, 0, EXPANDING, MINIMUM)
    }

    /// Create an expanding vertical spacer (pushes widgets apart).
    pub fn vertical_expanding() -> Self {
        Self::new(0, 0, MINIMUM, EXPANDING)
    }

    /// Change the spacer's size and policy at runtime.
    pub fn change_size(&self, w: i32, h: i32, h_policy: i32, v_policy: i32) {
        debug_assert!(!self.ptr.is_null());
        unsafe { ffi::QSpacerItem_changeSize(self.ptr, w, h, h_policy, v_policy); }
    }

    /// Return the raw pointer (for internal use by layouts).
    #[doc(hidden)]
    pub fn spacer_ptr(&self) -> *mut ffi::QSpacerItem {
        self.ptr
    }
}

impl Drop for Spacer {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            unsafe { ffi::QSpacerItem_delete(self.ptr); }
            self.ptr = std::ptr::null_mut();
        }
    }
}

/// Extension trait for adding spacers to box layouts.
pub trait SpacerExt {
    /// Add a spacer to this box layout.
    fn add_spacer(&mut self, spacer: Spacer);
}

impl SpacerExt for VBoxLayout {
    fn add_spacer(&mut self, spacer: Spacer) {
        unsafe { ffi::QVBoxLayout_addSpacerItem(self.layout_ptr(), spacer.spacer_ptr()); }
        // Spacer is now owned by the layout — prevent double-free
        std::mem::forget(spacer);
    }
}

impl SpacerExt for HBoxLayout {
    fn add_spacer(&mut self, spacer: Spacer) {
        unsafe { ffi::QHBoxLayout_addSpacerItem(self.layout_ptr(), spacer.spacer_ptr()); }
        std::mem::forget(spacer);
    }
}
