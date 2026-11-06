//! What Windows says about how a window should look and move (TD-0009 §5.1, §5.2; ADR-0007 §2):
//! light or dark, high contrast and its colors, transparency, remote sessions, battery saver and
//! animations. A window reads it at creation and again whenever Windows signals a change
//! (`WM_SETTINGCHANGE`, `WM_SYSCOLORCHANGE`, `WM_THEMECHANGED`, `WM_WTSSESSION_CHANGE`,
//! `PBT_POWERSETTINGCHANGE`), and tells its handler only if something here changed.
//!
//! Each fact comes from the Win32 source that needs no `WinRT`, so it reads the same in `WinPE` (no
//! `twinapi.appcore`):
//! - **dark:** `AppsUseLightTheme` is 0, and **transparency:** `EnableTransparency` isn't 0, both
//!   under `HKCU\…\Themes\Personalize`; `ImmersiveColorSet` signals both (S-01 measured). They are
//!   the values behind `UISettings`' app mode and `AdvancedEffectsEnabled`. A missing value is
//!   Windows' default: light, transparency on.
//! - **high contrast:** `SPI_GETHIGHCONTRAST`'s `HCF_HIGHCONTRASTON`, with `GetSysColor`'s colors.
//! - **animations:** `SPI_GETCLIENTAREAANIMATION`, the *Animation effects* switch that
//!   `UISettings.AnimationsEnabled` reports.
//! - **remote:** `GetSystemMetrics(SM_REMOTESESSION)`.
//! - **caret:** `GetCaretBlinkTime` and `SPI_GETCARETWIDTH`, the text cursor's blink rate and
//!   thickness (Settings › Accessibility › Text cursor). *(verify that changing the blink rate
//!   sends `WM_SETTINGCHANGE`; the thickness does.)*
//! - **battery saver:** `GetSystemPowerStatus`'s `SystemStatusFlag`; the window subscribes to
//!   `GUID_POWER_SAVING_STATUS`. *(verify on Windows 11 24H2+, whose Energy saver replaced it.)*
//! - **winpe:** `HKLM\SYSTEM\CurrentControlSet\Control\MiniNT` exists; **build:**
//!   `CurrentBuildNumber`. Neither changes while the process runs, so they're read once.

use std::{ffi::c_void, sync::OnceLock};

use windows::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
use windows_sys::{
    Win32::{
        Graphics::Gdi::{
            COLOR_BTNFACE, COLOR_BTNTEXT, COLOR_GRAYTEXT, COLOR_HIGHLIGHT, COLOR_HIGHLIGHTTEXT,
            COLOR_HOTLIGHT, COLOR_WINDOW, COLOR_WINDOWTEXT, GetSysColor, SYS_COLOR_INDEX,
        },
        System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS},
        UI::WindowsAndMessaging::{
            GetCaretBlinkTime, GetSystemMetrics, SM_REMOTESESSION, SPI_GETCARETWIDTH,
            SPI_GETCLIENTAREAANIMATION, SPI_GETHIGHCONTRAST, SystemParametersInfoW,
        },
    },
    core::{BOOL, GUID},
};

use crate::reg::{Access, RegKey, RegValue};

/// `WinNT.h`'s `GUID_POWER_SAVING_STATUS`: battery saver turned on or off. windows-sys files it
/// under `Win32_System_SystemServices`, a large feature for one constant.
pub(super) static GUID_POWER_SAVING_STATUS: GUID =
    GUID::from_u128(0xe00958c0_c213_4ace_ac77_fecced2eeea5);

const PERSONALIZE: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";
const MININT: &str = r"SYSTEM\CurrentControlSet\Control\MiniNT";
const CURRENT_VERSION: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";

/// The system colors high contrast shows, as `0xRRGGBB`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct SysColors {
    pub window: u32,
    pub window_text: u32,
    pub highlight: u32,
    pub highlight_text: u32,
    pub hot_light: u32,
    pub gray_text: u32,
    pub btn_face: u32,
    pub btn_text: u32,
}

/// One reading of the facts in the module docs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "one flag per setting Windows reports; they vary independently"
)]
pub struct Env {
    pub winpe: bool,
    /// The Windows build number; 0 if it couldn't be read.
    pub build: u32,
    /// Apps are dark (Settings › Personalization › Colors).
    pub dark: bool,
    /// *Transparency effects.*
    pub transparency: bool,
    pub high_contrast: bool,
    /// The system colors; high contrast's are the only colors it shows.
    pub colors: SysColors,
    /// *Animation effects*; off is reduced motion.
    pub animations: bool,
    /// A remote desktop session.
    pub remote: bool,
    pub battery_saver: bool,
    /// How long the caret is on, then off, in milliseconds; 0 doesn't blink.
    pub caret_blink_ms: u32,
    /// The caret's width in pixels at 100 %; at least 1.
    pub caret_width: u32,
}

impl Env {
    /// Reads every fact now: a few registry values and system parameters.
    #[must_use]
    pub fn read() -> Self {
        let (winpe, build) = fixed();
        let personalize = RegKey::current_user().open(PERSONALIZE, Access::Read).ok();
        let flag = |name: &str, default: bool| {
            personalize
                .as_ref()
                .and_then(|key| key.get(name).ok().flatten())
                .map_or(default, |value| !matches!(value, RegValue::Dword(0)))
        };
        Self {
            winpe,
            build,
            dark: !flag("AppsUseLightTheme", true),
            transparency: flag("EnableTransparency", true),
            high_contrast: high_contrast(),
            colors: sys_colors(),
            animations: animations(),
            // SAFETY: no preconditions.
            remote: unsafe { GetSystemMetrics(SM_REMOTESESSION) } != 0,
            battery_saver: battery_saver(),
            caret_blink_ms: caret_blink_ms(),
            caret_width: caret_width(),
        }
    }
}

/// `GetCaretBlinkTime`: `INFINITE` is no blinking (0), and a failure (0) Windows' default.
fn caret_blink_ms() -> u32 {
    // SAFETY: no preconditions.
    match unsafe { GetCaretBlinkTime() } {
        u32::MAX => 0,
        0 => 530,
        ms => ms,
    }
}

/// `SPI_GETCARETWIDTH`; 1, Windows' default, if it can't be read.
fn caret_width() -> u32 {
    let mut width: u32 = 0;
    // SAFETY: a live out-pointer to the DWORD SPI_GETCARETWIDTH writes.
    let ok = unsafe { SystemParametersInfoW(SPI_GETCARETWIDTH, 0, (&raw mut width).cast(), 0) };
    if ok != 0 && width > 0 { width } else { 1 }
}

/// `WinPE` and the build, read once.
fn fixed() -> (bool, u32) {
    static FIXED: OnceLock<(bool, u32)> = OnceLock::new();
    *FIXED.get_or_init(|| {
        let hklm = RegKey::local_machine();
        let winpe = hklm.open(MININT, Access::Read).is_ok();
        let build = hklm
            .open(CURRENT_VERSION, Access::Read)
            .ok()
            .and_then(|key| key.get("CurrentBuildNumber").ok().flatten())
            .and_then(|value| match value {
                RegValue::Sz(s) => s.to_str()?.trim().parse().ok(),
                _ => None,
            })
            .unwrap_or(0);
        (winpe, build)
    })
}

fn high_contrast() -> bool {
    let mut hc = HIGHCONTRASTW {
        cbSize: u32::try_from(size_of::<HIGHCONTRASTW>()).unwrap_or(0),
        ..HIGHCONTRASTW::default()
    };
    // SAFETY: hc is a live HIGHCONTRASTW whose cbSize says so; the call writes only into it.
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            hc.cbSize,
            (&raw mut hc).cast::<c_void>(),
            0,
        )
    };
    ok != 0 && hc.dwFlags.contains(HCF_HIGHCONTRASTON)
}

fn animations() -> bool {
    let mut on: BOOL = 1;
    // SAFETY: SPI_GETCLIENTAREAANIMATION writes one BOOL to the live local.
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            (&raw mut on).cast::<c_void>(),
            0,
        )
    };
    // Unknown (the call failed): Windows' default, on.
    ok == 0 || on != 0
}

fn battery_saver() -> bool {
    // SAFETY: plain data; all-zero is a valid value.
    let mut status: SYSTEM_POWER_STATUS = unsafe { std::mem::zeroed() };
    // SAFETY: status is a live local the call fills.
    let ok = unsafe { GetSystemPowerStatus(&raw mut status) };
    ok != 0 && status.SystemStatusFlag == 1
}

fn sys_colors() -> SysColors {
    let rgb = |index: SYS_COLOR_INDEX| {
        // SAFETY: no preconditions; every index here is a defined system color.
        rgb_of(unsafe { GetSysColor(index) })
    };
    SysColors {
        window: rgb(COLOR_WINDOW),
        window_text: rgb(COLOR_WINDOWTEXT),
        highlight: rgb(COLOR_HIGHLIGHT),
        highlight_text: rgb(COLOR_HIGHLIGHTTEXT),
        hot_light: rgb(COLOR_HOTLIGHT),
        gray_text: rgb(COLOR_GRAYTEXT),
        btn_face: rgb(COLOR_BTNFACE),
        btn_text: rgb(COLOR_BTNTEXT),
    }
}

/// A `COLORREF` (`0x00BBGGRR`) as `0xRRGGBB`.
const fn rgb_of(colorref: u32) -> u32 {
    let [r, g, b, _] = colorref.to_le_bytes();
    u32::from_be_bytes([0, r, g, b])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_colorref_reads_as_rgb() {
        assert_eq!(rgb_of(0x00_33_22_11), 0x11_22_33);
        assert_eq!(rgb_of(0x00_FF_FF_FF), 0xFF_FF_FF);
        assert_eq!(rgb_of(0), 0);
    }

    /// T2/T3: the reading works where the tests run, and agrees with itself. The runner's values
    /// go to the log for TD-0009 §13.
    #[test]
    fn the_environment_reads_and_is_stable() {
        let env = Env::read();
        println!("{env:?}");
        assert!(!env.winpe, "tests don't run in WinPE");
        assert_eq!(Env::read(), env);
        // Window text and window background are never the same color.
        assert_ne!(env.colors.window, env.colors.window_text);
    }
}
