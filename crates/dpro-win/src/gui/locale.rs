//! The user's formats for a table's sizes, days and moments (TD-0009 A2.5, slice 5b3).

#[cfg(test)]
mod tests {
    use dpro_core::units::UnixMillis;

    use super::*;

    /// 2026-11-06 15:04:00 UTC.
    const NOV_6_1504: u64 = 1_793_977_440_000;
    /// 2026-11-06, the day.
    const NOV_6: u64 = 1_793_923_200_000;

    fn date(l: &Locale, ms: u64) -> String {
        let mut out = String::new();
        l.date(UnixMillis(ms), &mut out);
        out
    }

    fn utc(l: &Locale, ms: u64) -> String {
        let mut out = String::new();
        l.moment(UnixMillis(ms), Zone::Utc, &mut out);
        out
    }

    /// T2/T3: a named locale's formats, as Windows (and Wine) write them.
    #[test]
    fn en_us_reads_as_windows_writes_it() {
        let l = Locale::named("en-US").expect("en-US");
        assert_eq!(l.decimal(), ".");
        assert_eq!(date(&l, NOV_6), "11/6/2026");
        assert_eq!(utc(&l, NOV_6_1504), "11/6/2026 3:04 PM");
    }

    #[test]
    fn de_de_has_its_own_separators() {
        let l = Locale::named("de-DE").expect("de-DE");
        assert_eq!(l.decimal(), ",");
        assert_eq!(date(&l, NOV_6), "06.11.2026");
        assert_eq!(utc(&l, NOV_6_1504), "06.11.2026 15:04");
    }

    #[test]
    fn a_name_windows_doesn_t_know_is_none() {
        assert!(Locale::named("xx-NOWHERE").is_none());
        assert!(Locale::named("").is_none());
    }

    /// The runner's own formats: printed, for TD-0009 §13.
    #[test]
    fn the_user_s_formats_read() {
        let l = Locale::user();
        let mut moment = String::new();
        l.time(UnixMillis(NOV_6_1504), &mut moment);
        println!(
            "user formats: decimal {:?}, day {:?}, moment {:?}",
            l.decimal(),
            date(&l, NOV_6),
            moment
        );
        assert!(!l.decimal().is_empty());
        assert!(!date(&l, NOV_6).is_empty());
        assert!(!moment.is_empty());
    }

    /// A day is that day anywhere: no time zone moves it.
    #[test]
    fn a_day_is_the_same_day_in_every_zone() {
        let l = Locale::named("en-US").expect("en-US");
        // Its last millisecond is still the 6th.
        assert_eq!(date(&l, NOV_6 + 86_399_999), "11/6/2026");
    }

    /// The zone's rules applied to a moment and back give the moment (to the millisecond).
    #[test]
    fn a_moment_in_local_time_converts_back_to_itself() {
        for ms in [0, NOV_6_1504, 1_720_000_000_123] {
            let local = local_time(UnixMillis(ms), Zone::Local).expect("a local time");
            assert_eq!(utc_of(&local), Some(UnixMillis(ms)), "{ms}");
        }
    }

    /// Past SYSTEMTIME's last year (30827), nothing is written.
    #[test]
    fn a_moment_windows_can_t_show_appends_nothing() {
        let l = Locale::named("en-US").expect("en-US");
        assert_eq!(date(&l, u64::MAX), "");
        assert_eq!(utc(&l, u64::MAX), "");
        let mut out = String::from("kept");
        l.time(UnixMillis(u64::MAX), &mut out);
        assert_eq!(out, "kept");
    }

    /// The stamp changes only when a format or the zone does.
    #[test]
    fn the_stamp_is_the_same_until_a_format_changes() {
        let a = stamp();
        println!("locale stamp {a:#018x}");
        assert_eq!(stamp(), a);
    }
}
