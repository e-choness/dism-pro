//! Which materials a window shows (ADR-0007 §2, TD-0009 §5.1): glass, or the solid fallback and
//! why.
//!
//! [`materials`] is ADR-0007's rule as a pure function of the [`Environment`] and the user's
//! [`GlassPref`], so it's tested exhaustively and re-run on every environment change: transparency,
//! high contrast, remote sessions and battery saver all change while the app runs.
//!
//! **Precedence.** When several reasons hold, the most lasting one is reported, so Settings ›
//! Appearance's line ("Glass is off: …") names what would still stand after the user changed what
//! they can: what the machine can't do ([`SolidReason::WinPe`], [`SolidReason::WindowsTooOld`],
//! [`SolidReason::NoHardwareGpu`], [`SolidReason::DeviceLost`]), then high contrast, then the
//! user's own choice, then Windows' settings (transparency, remote session, battery saver).

/// The first Windows build with the system backdrop API (`DWMWA_SYSTEMBACKDROP_TYPE`): Windows 11
/// 22H2 (ADR-0007 §1).
pub const GLASS_BUILD: u32 = 22_621;

/// How a window reaches the screen (TD-0009 §5.2). Chosen once, at window creation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PresenterKind {
    /// A DIB section copied by GDI: always works, never composes glass.
    Gdi,
    /// A `DirectComposition` swap chain, on a hardware device or WARP.
    Composition,
}

/// The user's glass preference: Settings › Appearance (TD-0009 §4.5), or the hidden
/// `--materials` test flag.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum GlassPref {
    /// Glass when the machine and Windows' settings allow it.
    #[default]
    Auto,
    /// Always solid.
    Off,
    /// `--materials glass`, for G1: glass whenever the presenter can compose, on WARP too, whatever
    /// Windows' transparency, remote and battery settings say. High contrast still wins.
    Force,
}

/// Why a window is solid. Each has a message, `ui-solid-reason-*` ([`SolidReason::message_id`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SolidReason {
    /// Windows PE: no DWM backdrop, no Direct3D device.
    WinPe,
    /// Before [`GLASS_BUILD`]: Windows 10 and Windows 11 21H2 (no undocumented APIs, ADR-0007 §4).
    WindowsTooOld { build: u32 },
    /// No hardware Direct3D device (WARP is a software renderer), or the GDI presenter.
    NoHardwareGpu,
    /// The graphics device was lost twice within a minute, and the window went on with WARP.
    DeviceLost,
    /// High contrast: system colors only, no materials (TD-0009 §4.4).
    HighContrast,
    /// Settings › Appearance › Glass is Off.
    UserChoice,
    /// Windows' *Transparency effects* are off.
    TransparencyOff,
    /// A remote desktop session.
    RemoteSession,
    /// Battery saver is on.
    BatterySaver,
}

impl SolidReason {
    /// The Fluent ID of the line that explains it (`i18n/en-US/ui.ftl`).
    #[must_use]
    pub const fn message_id(self) -> &'static str {
        match self {
            Self::WinPe => "ui-solid-reason-winpe",
            Self::WindowsTooOld { .. } => "ui-solid-reason-windows-too-old",
            Self::NoHardwareGpu => "ui-solid-reason-no-hardware-gpu",
            Self::DeviceLost => "ui-solid-reason-device-lost",
            Self::HighContrast => "ui-solid-reason-high-contrast",
            Self::UserChoice => "ui-solid-reason-user-choice",
            Self::TransparencyOff => "ui-solid-reason-transparency-off",
            Self::RemoteSession => "ui-solid-reason-remote-session",
            Self::BatterySaver => "ui-solid-reason-battery-saver",
        }
    }
}

/// What a window shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Materials {
    /// Mica, layers and Acrylic popups. The drawers' in-app acrylic blurs on the GPU, so only a
    /// hardware device draws it; on WARP a drawer gets the solid layer.
    Glass {
        in_app_acrylic: bool,
    },
    Solid(SolidReason),
}

/// What the machine and Windows' settings say, as far as materials go.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "one flag per fact ADR-0007 §2 names; they vary independently"
)]
pub struct Environment {
    pub winpe: bool,
    /// The Windows build number (`CurrentBuildNumber`).
    pub build: u32,
    pub presenter: PresenterKind,
    /// The composition presenter runs on a hardware device (not WARP).
    pub hardware_gpu: bool,
    /// The presenter lost its device twice within a minute and went on with WARP (TD-0009 §5.2).
    pub device_lost: bool,
    /// Windows' *Transparency effects*.
    pub transparency: bool,
    pub high_contrast: bool,
    /// A remote desktop session.
    pub remote: bool,
    pub battery_saver: bool,
}

/// ADR-0007 §2: glass, or the solid fallback and its most lasting reason (see the module docs).
#[must_use]
pub const fn materials(env: &Environment, pref: GlassPref) -> Materials {
    let forced = matches!(pref, GlassPref::Force);
    let reason = if env.winpe {
        SolidReason::WinPe
    } else if env.build < GLASS_BUILD {
        SolidReason::WindowsTooOld { build: env.build }
    } else if matches!(env.presenter, PresenterKind::Gdi) || !(env.hardware_gpu || forced) {
        SolidReason::NoHardwareGpu
    } else if env.device_lost && !forced {
        SolidReason::DeviceLost
    } else if env.high_contrast {
        SolidReason::HighContrast
    } else if matches!(pref, GlassPref::Off) {
        SolidReason::UserChoice
    } else if !env.transparency && !forced {
        SolidReason::TransparencyOff
    } else if env.remote && !forced {
        SolidReason::RemoteSession
    } else if env.battery_saver && !forced {
        SolidReason::BatterySaver
    } else {
        return Materials::Glass {
            in_app_acrylic: env.hardware_gpu && !env.device_lost,
        };
    };
    Materials::Solid(reason)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A Windows 11 24H2 desktop with a GPU and default settings: glass.
    const GLASSY: Environment = Environment {
        winpe: false,
        build: 26_100,
        presenter: PresenterKind::Composition,
        hardware_gpu: true,
        device_lost: false,
        transparency: true,
        high_contrast: false,
        remote: false,
        battery_saver: false,
    };

    const PREFS: [GlassPref; 3] = [GlassPref::Auto, GlassPref::Off, GlassPref::Force];

    /// Every environment: each flag both ways, both presenters, and builds either side of
    /// [`GLASS_BUILD`] (Windows 10 22H2, Windows 11 21H2, the build before, 22H2, 24H2).
    fn every_environment() -> Vec<Environment> {
        let mut all = Vec::new();
        for bits in 0_u32..1 << 7 {
            let bit = |n: u32| bits >> n & 1 == 1;
            for build in [19_045, 22_000, GLASS_BUILD - 1, GLASS_BUILD, 26_100] {
                for presenter in [PresenterKind::Gdi, PresenterKind::Composition] {
                    all.push(Environment {
                        winpe: bit(0),
                        build,
                        presenter,
                        hardware_gpu: bit(1),
                        device_lost: bit(2),
                        transparency: bit(3),
                        high_contrast: bit(4),
                        remote: bit(5),
                        battery_saver: bit(6),
                    });
                }
            }
        }
        all
    }

    /// ADR-0007 §2's blockers, each with the reason it gives and whether `Force` overrides it,
    /// listed in the module docs' precedence.
    fn blockers(env: &Environment, pref: GlassPref) -> Vec<(SolidReason, bool)> {
        let all = [
            (env.winpe, SolidReason::WinPe, false),
            (
                env.build < GLASS_BUILD,
                SolidReason::WindowsTooOld { build: env.build },
                false,
            ),
            (
                env.presenter == PresenterKind::Gdi,
                SolidReason::NoHardwareGpu,
                false,
            ),
            (!env.hardware_gpu, SolidReason::NoHardwareGpu, true),
            (env.device_lost, SolidReason::DeviceLost, true),
            (env.high_contrast, SolidReason::HighContrast, false),
            (pref == GlassPref::Off, SolidReason::UserChoice, false),
            (!env.transparency, SolidReason::TransparencyOff, true),
            (env.remote, SolidReason::RemoteSession, true),
            (env.battery_saver, SolidReason::BatterySaver, true),
        ];
        all.into_iter()
            .filter(|&(holds, _, overridable)| holds && !(overridable && pref == GlassPref::Force))
            .map(|(_, reason, overridable)| (reason, overridable))
            .collect()
    }

    /// TD-0009 §10: exhaustive over `Environment` × `GlassPref` (3,840 cases). Glass exactly when
    /// no blocker holds; otherwise solid for the first blocker in precedence order.
    #[test]
    fn the_truth_table_is_adr_0007_s_rule() {
        let envs = every_environment();
        assert_eq!(envs.len() * PREFS.len(), 3_840);
        let mut glass = 0;
        for env in &envs {
            for pref in PREFS {
                let got = materials(env, pref);
                match blockers(env, pref).first() {
                    None => {
                        glass += 1;
                        assert_eq!(
                            got,
                            Materials::Glass {
                                in_app_acrylic: env.hardware_gpu && !env.device_lost
                            },
                            "{env:?} {pref:?}"
                        );
                    }
                    Some(&(reason, _)) => {
                        assert_eq!(got, Materials::Solid(reason), "{env:?} {pref:?}");
                    }
                }
            }
        }
        // Auto: only the glassy row's 2 builds. Force: any GPU state, loss, transparency,
        // remote and battery saver (2 builds × 2^5).
        assert_eq!(glass, 2 + 2 * 32);
    }

    /// Turning on any blocker never brings glass back: the rule is monotonic.
    #[test]
    fn a_blocker_never_brings_glass() {
        let solid = |env: &Environment, pref| matches!(materials(env, pref), Materials::Solid(_));
        for env in every_environment() {
            for pref in PREFS {
                let worse = [
                    Environment { winpe: true, ..env },
                    Environment {
                        build: 19_045,
                        ..env
                    },
                    Environment {
                        presenter: PresenterKind::Gdi,
                        ..env
                    },
                    Environment {
                        hardware_gpu: false,
                        ..env
                    },
                    Environment {
                        device_lost: true,
                        ..env
                    },
                    Environment {
                        transparency: false,
                        ..env
                    },
                    Environment {
                        high_contrast: true,
                        ..env
                    },
                    Environment {
                        remote: true,
                        ..env
                    },
                    Environment {
                        battery_saver: true,
                        ..env
                    },
                ];
                for w in worse {
                    assert!(!solid(&env, pref) || solid(&w, pref), "{env:?} → {w:?}");
                }
            }
            // Off is always solid, and Force shows glass wherever Auto does.
            assert!(solid(&env, GlassPref::Off), "{env:?}");
            if !solid(&env, GlassPref::Auto) {
                assert!(!solid(&env, GlassPref::Force), "{env:?}");
            }
        }
    }

    #[test]
    fn named_cases() {
        let auto = |env| materials(&env, GlassPref::Auto);
        assert_eq!(
            auto(GLASSY),
            Materials::Glass {
                in_app_acrylic: true
            }
        );
        // A GPU-less runner: Auto is solid, Force composes on WARP without in-app acrylic.
        let warp = Environment {
            hardware_gpu: false,
            ..GLASSY
        };
        assert_eq!(auto(warp), Materials::Solid(SolidReason::NoHardwareGpu));
        assert_eq!(
            materials(&warp, GlassPref::Force),
            Materials::Glass {
                in_app_acrylic: false
            }
        );
        // WinPE is also too old and GDI-only, and says WinPE.
        let pe = Environment {
            winpe: true,
            build: 19_041,
            presenter: PresenterKind::Gdi,
            hardware_gpu: false,
            ..GLASSY
        };
        assert_eq!(auto(pe), Materials::Solid(SolidReason::WinPe));
        assert_eq!(
            auto(Environment {
                build: 19_045,
                ..GLASSY
            }),
            Materials::Solid(SolidReason::WindowsTooOld { build: 19_045 })
        );
        // High contrast wins over the user's choice and Force; the user's choice over Windows'.
        let hc = Environment {
            high_contrast: true,
            transparency: false,
            ..GLASSY
        };
        for pref in PREFS {
            assert_eq!(
                materials(&hc, pref),
                Materials::Solid(SolidReason::HighContrast)
            );
        }
        let plain = Environment {
            transparency: false,
            ..GLASSY
        };
        assert_eq!(
            materials(&plain, GlassPref::Off),
            Materials::Solid(SolidReason::UserChoice)
        );
        assert_eq!(auto(plain), Materials::Solid(SolidReason::TransparencyOff));
        assert_eq!(
            auto(Environment {
                remote: true,
                battery_saver: true,
                ..GLASSY
            }),
            Materials::Solid(SolidReason::RemoteSession)
        );
    }

    /// Every reason has its line in the source locale.
    #[test]
    fn every_reason_has_a_message() {
        let ftl = include_str!("../../../i18n/en-US/ui.ftl");
        for reason in [
            SolidReason::WinPe,
            SolidReason::WindowsTooOld { build: 19_045 },
            SolidReason::NoHardwareGpu,
            SolidReason::DeviceLost,
            SolidReason::HighContrast,
            SolidReason::UserChoice,
            SolidReason::TransparencyOff,
            SolidReason::RemoteSession,
            SolidReason::BatterySaver,
        ] {
            let id = reason.message_id();
            assert!(
                ftl.lines().any(|l| l.starts_with(&format!("{id} ="))),
                "{id} isn't in ui.ftl"
            );
        }
    }
}
