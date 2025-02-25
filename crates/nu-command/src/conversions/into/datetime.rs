use crate::input_handler::{operate, CmdArgument};
use crate::{generate_strftime_list, parse_date_from_string};
use chrono::{DateTime, FixedOffset, Local, LocalResult, TimeZone, Utc};
use nu_engine::CallExt;
use nu_protocol::ast::Call;
use nu_protocol::ast::CellPath;
use nu_protocol::engine::{Command, EngineState, Stack};
use nu_protocol::{
    Category, Example, IntoPipelineData, PipelineData, ShellError, Signature, Span, Spanned,
    SyntaxShape, Type, Value,
};

struct Arguments {
    zone_options: Option<Spanned<Zone>>,
    format_options: Option<DatetimeFormat>,
    cell_paths: Option<Vec<CellPath>>,
}

impl CmdArgument for Arguments {
    fn take_cell_paths(&mut self) -> Option<Vec<CellPath>> {
        self.cell_paths.take()
    }
}

// In case it may be confused with chrono::TimeZone
#[derive(Clone, Debug)]
enum Zone {
    Utc,
    Local,
    East(u8),
    West(u8),
    Error, // we want Nushell to cast it instead of Rust
}

impl Zone {
    fn new(i: i64) -> Self {
        if i.abs() <= 12 {
            // guaranteed here
            if i >= 0 {
                Self::East(i as u8) // won't go out of range
            } else {
                Self::West(-i as u8) // same here
            }
        } else {
            Self::Error // Out of range
        }
    }
    fn from_string(s: String) -> Self {
        match s.to_lowercase().as_str() {
            "utc" | "u" => Self::Utc,
            "local" | "l" => Self::Local,
            _ => Self::Error,
        }
    }
}

#[derive(Clone)]
pub struct SubCommand;

impl Command for SubCommand {
    fn name(&self) -> &str {
        "into datetime"
    }

    fn signature(&self) -> Signature {
        Signature::build("into datetime")
        .input_output_types(vec![
            (Type::Int, Type::Date),
            (Type::String, Type::Date),
        ])
        .named(
                "timezone",
                SyntaxShape::String,
                "Specify timezone if the input is a Unix timestamp. Valid options: 'UTC' ('u') or 'LOCAL' ('l')",
                Some('z'),
            )
            .named(
                "offset",
                SyntaxShape::Int,
                "Specify timezone by offset from UTC if the input is a Unix timestamp, like '+8', '-4'",
                Some('o'),
            )
            .named(
                "format",
                SyntaxShape::String,
                "Specify an expected format for parsing strings to datetimes. Use --list to see all possible options",
                Some('f'),
            )
            .switch(
                "list",
                "Show all possible variables for use with the --format flag",
                Some('l'),
                )
            .rest(
            "rest",
                SyntaxShape::CellPath,
                "for a data structure input, convert data at the given cell paths",
            )
            .category(Category::Conversions)
    }

    fn run(
        &self,
        engine_state: &EngineState,
        stack: &mut Stack,
        call: &Call,
        input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        if call.has_flag("list") {
            Ok(generate_strftime_list(call.head, true).into_pipeline_data())
        } else {
            let cell_paths = call.rest(engine_state, stack, 0)?;
            let cell_paths = (!cell_paths.is_empty()).then_some(cell_paths);

            // if zone-offset is specified, then zone will be neglected
            let timezone = call.get_flag::<Spanned<String>>(engine_state, stack, "timezone")?;
            let zone_options =
                match &call.get_flag::<Spanned<i64>>(engine_state, stack, "offset")? {
                    Some(zone_offset) => Some(Spanned {
                        item: Zone::new(zone_offset.item),
                        span: zone_offset.span,
                    }),
                    None => timezone.as_ref().map(|zone| Spanned {
                        item: Zone::from_string(zone.item.clone()),
                        span: zone.span,
                    }),
                };
            let format_options = call
                .get_flag::<String>(engine_state, stack, "format")?
                .as_ref()
                .map(|fmt| DatetimeFormat(fmt.to_string()));
            let args = Arguments {
                format_options,
                zone_options,
                cell_paths,
            };
            operate(action, args, input, call.head, engine_state.ctrlc.clone())
        }
    }

    fn usage(&self) -> &str {
        "Convert text into a datetime."
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["convert", "timezone", "UTC"]
    }

    fn examples(&self) -> Vec<Example> {
        let example_result_1 = |secs: i64, nsecs: u32| match Utc.timestamp_opt(secs, nsecs) {
            LocalResult::Single(dt) => Some(Value::Date {
                val: dt.into(),
                span: Span::test_data(),
            }),
            _ => panic!("datetime: help example is invalid"),
        };
        let example_result_2 = |millis: i64| match Utc.timestamp_millis_opt(millis) {
            LocalResult::Single(dt) => Some(Value::Date {
                val: dt.into(),
                span: Span::test_data(),
            }),
            _ => panic!("datetime: help example is invalid"),
        };
        vec![
            Example {
                description: "Convert to datetime",
                example: "'27.02.2021 1:55 pm +0000' | into datetime",
                result: example_result_1(1614434100,0)
            },
            Example {
                description: "Convert to datetime",
                example: "'2021-02-27T13:55:40+00:00' | into datetime",
                result: example_result_1(1614434140, 0)
            },
            Example {
                description: "Convert to datetime using a custom format",
                example: "'20210227_135540+0000' | into datetime -f '%Y%m%d_%H%M%S%z'",
                result: example_result_1(1614434140, 0)

            },
            Example {
                description: "Convert timestamp (no larger than 8e+12) to a UTC datetime",
                example: "1614434140 | into datetime",
                result: example_result_1(1614434140, 0)
            },
            Example {
                description:
                    "Convert timestamp (no larger than 8e+12) to datetime using a specified timezone offset (between -12 and 12)",
                example: "1614434140 | into datetime -o +9",
                result: None,
            },
            Example {
                description:
                    "Convert a millisecond-precise timestamp",
                example: "1656165681720 | into datetime",
                result: example_result_2(1656165681720)
            },
        ]
    }
}

#[derive(Clone)]
struct DatetimeFormat(String);

fn action(input: &Value, args: &Arguments, head: Span) -> Value {
    let timezone = &args.zone_options;
    let dateformat = &args.format_options;
    // Check to see if input looks like a Unix timestamp (i.e. can it be parsed to an int?)
    let timestamp = match input {
        Value::Int { val, .. } => Ok(*val),
        Value::String { val, .. } => val.parse::<i64>(),
        // Propagate errors by explicitly matching them before the final case.
        Value::Error { .. } => return input.clone(),
        other => {
            return Value::Error {
                error: ShellError::OnlySupportsThisInputType {
                    exp_input_type: "string and integer".into(),
                    wrong_type: other.get_type().to_string(),
                    dst_span: head,
                    src_span: other.expect_span(),
                },
            };
        }
    };

    if let Ok(ts) = timestamp {
        const TIMESTAMP_BOUND: i64 = 8.2e+12 as i64;
        const HOUR: i32 = 3600;

        if ts.abs() > TIMESTAMP_BOUND {
            return Value::Error {
                error: ShellError::UnsupportedInput(
                    "timestamp is out of range; it should between -8e+12 and 8e+12".to_string(),
                    format!("timestamp is {ts:?}"),
                    head,
                    // Again, can safely unwrap this from here on
                    input.expect_span(),
                ),
            };
        }

        macro_rules! match_datetime {
            ($expr:expr) => {
                match $expr {
                    LocalResult::Single(dt) => Value::Date {
                        val: dt.into(),
                        span: head,
                    },
                    _ => {
                        return Value::Error {
                            error: ShellError::UnsupportedInput(
                                "The given local datetime representation is invalid.".into(),
                                format!("timestamp is {:?}", ts),
                                head,
                                head,
                            ),
                        };
                    }
                }
            };
        }

        return match timezone {
            // default to UTC
            None => {
                // be able to convert chrono::Utc::now()
                match ts.to_string().len() {
                    x if x > 13 => Value::Date {
                        val: Utc.timestamp_nanos(ts).into(),
                        span: head,
                    },
                    x if x > 10 => match_datetime!(Utc.timestamp_millis_opt(ts)),
                    _ => match_datetime!(Utc.timestamp_opt(ts, 0)),
                }
            }
            Some(Spanned { item, span }) => match item {
                Zone::Utc => match_datetime!(Utc.timestamp_opt(ts, 0)),
                Zone::Local => match_datetime!(Local.timestamp_opt(ts, 0)),
                Zone::East(i) => match FixedOffset::east_opt((*i as i32) * HOUR) {
                    Some(eastoffset) => match_datetime!(eastoffset.timestamp_opt(ts, 0)),
                    None => Value::Error {
                        error: ShellError::DatetimeParseError(input.debug_value(), *span),
                    },
                },
                Zone::West(i) => match FixedOffset::west_opt((*i as i32) * HOUR) {
                    Some(westoffset) => match_datetime!(westoffset.timestamp_opt(ts, 0)),
                    None => Value::Error {
                        error: ShellError::DatetimeParseError(input.debug_value(), *span),
                    },
                },
                Zone::Error => Value::Error {
                    // This is an argument error, not an input error
                    error: ShellError::TypeMismatch(
                        "Invalid timezone or offset".to_string(),
                        *span,
                    ),
                },
            },
        };
    }

    // If input is not a timestamp, try parsing it as a string
    match input {
        Value::String { val, span } => {
            match dateformat {
                Some(dt) => match DateTime::parse_from_str(val, &dt.0) {
                    Ok(d) => Value::Date { val: d, span: head },
                    Err(reason) => {
                        Value::Error {
                            error: ShellError::CantConvert(
                                format!("could not parse as datetime using format '{}'", dt.0),
                                reason.to_string(),
                                head,
                                Some("you can use `into datetime` without a format string to enable flexible parsing".to_string())
                            ),
                        }
                    }
                },
                // Tries to automatically parse the date
                // (i.e. without a format string)
                // and assumes the system's local timezone if none is specified
                None => match parse_date_from_string(val, *span) {
                    Ok(date) => Value::Date {
                        val: date,
                        span: *span,
                    },
                    Err(err) => err,
                },
            }
        }
        // Propagate errors by explicitly matching them before the final case.
        Value::Error { .. } => input.clone(),
        other => Value::Error {
            error: ShellError::OnlySupportsThisInputType {
                exp_input_type: "string".into(),
                wrong_type: other.get_type().to_string(),
                dst_span: head,
                src_span: other.expect_span(),
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::{action, DatetimeFormat, SubCommand, Zone};
    use nu_protocol::Type::Error;

    #[test]
    fn test_examples() {
        use crate::test_examples;

        test_examples(SubCommand {})
    }

    #[test]
    fn takes_a_date_format() {
        let date_str = Value::test_string("16.11.1984 8:00 am +0000");
        let fmt_options = Some(DatetimeFormat("%d.%m.%Y %H:%M %P %z".to_string()));
        let args = Arguments {
            zone_options: None,
            format_options: fmt_options,
            cell_paths: None,
        };
        let actual = action(&date_str, &args, Span::test_data());
        let expected = Value::Date {
            val: DateTime::parse_from_str("16.11.1984 8:00 am +0000", "%d.%m.%Y %H:%M %P %z")
                .unwrap(),
            span: Span::test_data(),
        };
        assert_eq!(actual, expected)
    }

    #[test]
    fn takes_iso8601_date_format() {
        let date_str = Value::test_string("2020-08-04T16:39:18+00:00");
        let args = Arguments {
            zone_options: None,
            format_options: None,
            cell_paths: None,
        };
        let actual = action(&date_str, &args, Span::test_data());
        let expected = Value::Date {
            val: DateTime::parse_from_str("2020-08-04T16:39:18+00:00", "%Y-%m-%dT%H:%M:%S%z")
                .unwrap(),
            span: Span::test_data(),
        };
        assert_eq!(actual, expected)
    }

    #[test]
    fn takes_timestamp_offset() {
        let date_str = Value::test_string("1614434140");
        let timezone_option = Some(Spanned {
            item: Zone::East(8),
            span: Span::test_data(),
        });
        let args = Arguments {
            zone_options: timezone_option,
            format_options: None,
            cell_paths: None,
        };
        let actual = action(&date_str, &args, Span::test_data());
        let expected = Value::Date {
            val: DateTime::parse_from_str("2021-02-27 21:55:40 +08:00", "%Y-%m-%d %H:%M:%S %z")
                .unwrap(),
            span: Span::test_data(),
        };

        assert_eq!(actual, expected)
    }

    #[test]
    fn takes_timestamp_offset_as_int() {
        let date_int = Value::test_int(1614434140);
        let timezone_option = Some(Spanned {
            item: Zone::East(8),
            span: Span::test_data(),
        });
        let args = Arguments {
            zone_options: timezone_option,
            format_options: None,
            cell_paths: None,
        };
        let actual = action(&date_int, &args, Span::test_data());
        let expected = Value::Date {
            val: DateTime::parse_from_str("2021-02-27 21:55:40 +08:00", "%Y-%m-%d %H:%M:%S %z")
                .unwrap(),
            span: Span::test_data(),
        };

        assert_eq!(actual, expected)
    }

    #[test]
    fn takes_timestamp() {
        let date_str = Value::test_string("1614434140");
        let timezone_option = Some(Spanned {
            item: Zone::Local,
            span: Span::test_data(),
        });
        let args = Arguments {
            zone_options: timezone_option,
            format_options: None,
            cell_paths: None,
        };
        let actual = action(&date_str, &args, Span::test_data());
        let expected = Value::Date {
            val: Local.timestamp_opt(1614434140, 0).unwrap().into(),
            span: Span::test_data(),
        };

        assert_eq!(actual, expected)
    }

    #[test]
    fn takes_timestamp_without_timezone() {
        let date_str = Value::test_string("1614434140");
        let args = Arguments {
            zone_options: None,
            format_options: None,
            cell_paths: None,
        };
        let actual = action(&date_str, &args, Span::test_data());

        let expected = Value::Date {
            val: Utc.timestamp_opt(1614434140, 0).unwrap().into(),
            span: Span::test_data(),
        };

        assert_eq!(actual, expected)
    }

    #[test]
    fn takes_invalid_timestamp() {
        let date_str = Value::test_string("10440970000000");
        let timezone_option = Some(Spanned {
            item: Zone::Utc,
            span: Span::test_data(),
        });
        let args = Arguments {
            zone_options: timezone_option,
            format_options: None,
            cell_paths: None,
        };
        let actual = action(&date_str, &args, Span::test_data());

        assert_eq!(actual.get_type(), Error);
    }

    #[test]
    fn communicates_parsing_error_given_an_invalid_datetimelike_string() {
        let date_str = Value::test_string("16.11.1984 8:00 am Oops0000");
        let fmt_options = Some(DatetimeFormat("%d.%m.%Y %H:%M %P %z".to_string()));
        let args = Arguments {
            zone_options: None,
            format_options: fmt_options,
            cell_paths: None,
        };
        let actual = action(&date_str, &args, Span::test_data());

        assert_eq!(actual.get_type(), Error);
    }
}
