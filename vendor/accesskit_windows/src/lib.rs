// Copyright 2021 The AccessKit Authors. All rights reserved.
// Licensed under the Apache License, Version 2.0 (found in
// the LICENSE-APACHE file) or the MIT license (found in
// the LICENSE-MIT file), at your option.

mod context;
mod filters;
mod node;
mod text;
mod util;
mod window_handle;

mod adapter;
pub use adapter::{Adapter, QueuedEvents};

// dpro: `SubclassingAdapter` (src/subclass.rs) is removed. dpro-win owns its window procedure,
// and the adapter's test needs winit (PATCHES.md, 1).

pub use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};

#[cfg(test)]
mod tests;
