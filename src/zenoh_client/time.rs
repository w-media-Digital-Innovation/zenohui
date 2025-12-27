use chrono::NaiveDateTime;
use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(untagged)]
pub enum Time {
    Unknown,
    Local(NaiveDateTime),
}

impl Time {
    pub fn new_now() -> Self {
        Self::Local(chrono::Local::now().naive_local())
    }

    pub const fn as_optional(&self) -> Option<&NaiveDateTime> {
        if let Self::Local(time) = self {
            Some(time)
        } else {
            None
        }
    }

    #[cfg(test)]
    pub fn datetime_example() -> NaiveDateTime {
        chrono::NaiveDate::from_ymd_opt(1996, 12, 19)
            .unwrap()
            .and_hms_opt(16, 39, 57)
            .unwrap()
    }
}

impl std::fmt::Display for Time {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown => fmt.pad("UNKNOWN"),
            Self::Local(time) => fmt.write_fmt(format_args!("{}", time.format("%_H:%M:%S.%3f"))),
        }
    }
}

#[test]
fn new_now_local() {
    let result = Time::new_now();
    dbg!(result);
    assert!(matches!(result, Time::Local(_)));
}

#[test]
fn optional_unknown() {
    let time = Time::Unknown;
    assert_eq!(time.as_optional(), None);
}

#[test]
fn optional_time() {
    let date = Time::datetime_example();
    let time = Time::Local(date);
    assert_eq!(time.as_optional(), Some(&date));
}

#[test]
fn local_to_string() {
    let date = Time::datetime_example();
    let time = Time::Local(date);
    assert_eq!(time.to_string(), "16:39:57.000");
}

#[test]
fn unknown_to_string() {
    let time = Time::Unknown;
    assert_eq!(time.to_string(), "UNKNOWN");
}

#[test]
fn unknown_fmt_width() {
    let time = Time::Unknown;
    let time = format!("{time:12}");
    assert_eq!(time, "UNKNOWN     ");
}
