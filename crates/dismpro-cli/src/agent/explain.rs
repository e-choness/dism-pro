//! `explain_error` (TD-0013 §5.3): what an error code means and what to try next. It runs in the
//! front, without the broker: the system's own text for the code, plus DISM Pro's diagnosis
//! when the health decode table knows it (TD-0006).

use dpro_cli_fmt::messages::Messages;
use dpro_engine::health::logs::diagnosis_for;
use serde_json::{Value, json};

/// `HRESULT_FROM_WIN32`'s facility bits.
const WIN32_FACILITY: u32 = 0x8007_0000;

/// A code as an agent may write it: `0x800f081f`, `-2146498529` or `5`.
///
/// # Errors
/// Anything else, worded for the agent.
pub(crate) fn parse_code(text: &str) -> Result<u32, String> {
    let t = text.trim();
    let parsed = if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        u32::from_str_radix(hex, 16).ok()
    } else {
        t.parse::<i64>().ok().and_then(|n| {
            // Negative decimals are HRESULTs as signed numbers (how .NET and PowerShell print them).
            i32::try_from(n)
                .map(i32::cast_unsigned)
                .ok()
                .or_else(|| u32::try_from(n).ok())
        })
    };
    parsed.ok_or_else(|| {
        format!(
            "{text:?} isn't an error code. Use hex like 0x800f081f, or decimal like -2146498529 or 5."
        )
    })
}

/// What `code` means. A plain Win32 code (below 0x10000) is shown as its HRESULT too.
/// `system_text` is the system's message for a code (`dpro_win::system::error_text`).
pub(crate) fn explain(
    code: u32,
    system_text: impl Fn(u32) -> Option<String>,
    m: &Messages,
) -> Value {
    let hresult = if code != 0 && code <= 0xFFFF {
        WIN32_FACILITY | code
    } else {
        code
    };
    let win32 = (hresult & 0xFFFF_0000 == WIN32_FACILITY).then_some(hresult & 0xFFFF);
    let diagnosis = diagnosis_for(hresult);
    let message = system_text(hresult).or_else(|| win32.and_then(&system_text));
    json!({
        "code": format!("{hresult:#010x}"),
        "decimal": hresult.cast_signed(),
        "failure": hresult & 0x8000_0000 != 0,
        "facility": (hresult >> 16) & 0x1FFF,
        "win32": win32,
        "message": message,
        "diagnosis": diagnosis.map(|(id, _)| id),
        "next": diagnosis.map(|(id, _)| m.text(id)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_parse_in_every_usual_form() {
        assert_eq!(parse_code("0x800f081f"), Ok(0x800F_081F));
        assert_eq!(parse_code(" 0X800F081F "), Ok(0x800F_081F));
        assert_eq!(parse_code("-2146498529"), Ok(0x800F_081F));
        assert_eq!(parse_code("2148468767"), Ok(0x800F_081F));
        assert_eq!(parse_code("5"), Ok(5));
        for bad in [
            "",
            "0x",
            "0xZZ",
            "five",
            "99999999999",
            "-99999999999",
            "0x1FFFFFFFF",
        ] {
            assert!(parse_code(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn known_codes_carry_the_diagnosis_and_next_step() {
        let m = Messages::en_us();
        let none = |_| None;
        let v = explain(0x800F_081F, none, &m);
        assert_eq!(v["code"], "0x800f081f");
        assert_eq!(v["decimal"], -2_146_498_529_i64);
        assert_eq!(v["diagnosis"], "health-finding-source-missing");
        assert_eq!(v["next"], m.text("health-finding-source-missing"));
        assert_eq!(v["win32"], Value::Null);
        assert_eq!(v["failure"], true);
    }

    #[test]
    fn win32_codes_are_shown_as_hresults_and_keep_the_system_text() {
        let m = Messages::en_us();
        let text = |c: u32| (c == 0x8007_0005).then(|| "Access is denied.".to_owned());
        let v = explain(5, text, &m);
        assert_eq!(v["code"], "0x80070005");
        assert_eq!(v["win32"], 5);
        assert_eq!(v["message"], "Access is denied.");
        assert_eq!(v["diagnosis"], "health-finding-access-denied");
        // Some systems only know the plain Win32 form.
        let plain = |c: u32| (c == 5).then(|| "Access is denied.".to_owned());
        assert_eq!(explain(5, plain, &m)["message"], "Access is denied.");
        assert_eq!(explain(0, |_| None, &m)["failure"], false);
    }
}
