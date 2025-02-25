use crate::input_handler::{operate, CmdArgument};
use nu_engine::CallExt;
use nu_protocol::{
    ast::{Call, CellPath},
    engine::{Command, EngineState, Stack},
    Category, Example, PipelineData, ShellError, Signature, Span, SyntaxShape, Type, Value,
};

struct Arguments {
    radix: u32,
    cell_paths: Option<Vec<CellPath>>,
    little_endian: bool,
}

impl CmdArgument for Arguments {
    fn take_cell_paths(&mut self) -> Option<Vec<CellPath>> {
        self.cell_paths.take()
    }
}

#[derive(Clone)]
pub struct SubCommand;

impl Command for SubCommand {
    fn name(&self) -> &str {
        "into int"
    }

    fn signature(&self) -> Signature {
        Signature::build("into int")
            .input_output_types(vec![
                (Type::String, Type::Int),
                (Type::Number, Type::Int),
                (Type::Bool, Type::Int),
                // Unix timestamp in seconds
                (Type::Date, Type::Int),
                // TODO: Users should do this by dividing a Filesize by a Filesize explicitly
                (Type::Filesize, Type::Int),
            ])
            .vectorizes_over_list(true)
            .named("radix", SyntaxShape::Number, "radix of integer", Some('r'))
            .switch("little-endian", "use little-endian byte decoding", None)
            .rest(
                "rest",
                SyntaxShape::CellPath,
                "for a data structure input, convert data at the given cell paths",
            )
            .category(Category::Conversions)
    }

    fn usage(&self) -> &str {
        "Convert value to integer."
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["convert", "number", "natural"]
    }

    fn run(
        &self,
        engine_state: &EngineState,
        stack: &mut Stack,
        call: &Call,
        input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        let cell_paths = call.rest(engine_state, stack, 0)?;
        let cell_paths = (!cell_paths.is_empty()).then_some(cell_paths);

        let radix = call.get_flag::<Value>(engine_state, stack, "radix")?;
        let radix: u32 = match radix {
            Some(Value::Int { val, span }) => {
                if !(2..=36).contains(&val) {
                    return Err(ShellError::TypeMismatch(
                        "Radix must lie in the range [2, 36]".to_string(),
                        span,
                    ));
                }
                val as u32
            }
            Some(_) => 10,
            None => 10,
        };
        let args = Arguments {
            radix,
            little_endian: call.has_flag("little-endian"),
            cell_paths,
        };
        operate(action, args, input, call.head, engine_state.ctrlc.clone())
    }

    fn examples(&self) -> Vec<Example> {
        vec![
            Example {
                description: "Convert string to integer in table",
                example: "[[num]; ['-5'] [4] [1.5]] | into int num",
                result: None,
            },
            Example {
                description: "Convert string to integer",
                example: "'2' | into int",
                result: Some(Value::test_int(2)),
            },
            Example {
                description: "Convert decimal to integer",
                example: "5.9 | into int",
                result: Some(Value::test_int(5)),
            },
            Example {
                description: "Convert decimal string to integer",
                example: "'5.9' | into int",
                result: Some(Value::test_int(5)),
            },
            Example {
                description: "Convert file size to integer",
                example: "4KB | into int",
                result: Some(Value::test_int(4000)),
            },
            Example {
                description: "Convert bool to integer",
                example: "[false, true] | into int",
                result: Some(Value::List {
                    vals: vec![Value::test_int(0), Value::test_int(1)],
                    span: Span::test_data(),
                }),
            },
            Example {
                description: "Convert date to integer (Unix timestamp)",
                example: "2022-02-02 | into int",
                result: Some(Value::test_int(1643760000)),
            },
            Example {
                description: "Convert to integer from binary",
                example: "'1101' | into int -r 2",
                result: Some(Value::test_int(13)),
            },
            Example {
                description: "Convert to integer from hex",
                example: "'FF' |  into int -r 16",
                result: Some(Value::test_int(255)),
            },
            Example {
                description: "Convert octal string to integer",
                example: "'0o10132' | into int",
                result: Some(Value::test_int(4186)),
            },
            Example {
                description: "Convert 0 padded string to integer",
                example: "'0010132' | into int",
                result: Some(Value::test_int(10132)),
            },
            Example {
                description: "Convert 0 padded string to integer with radix",
                example: "'0010132' | into int -r 8",
                result: Some(Value::test_int(4186)),
            },
        ]
    }
}

fn action(input: &Value, args: &Arguments, span: Span) -> Value {
    let radix = args.radix;
    let little_endian = args.little_endian;
    match input {
        Value::Int { val: _, .. } => {
            if radix == 10 {
                input.clone()
            } else {
                convert_int(input, span, radix)
            }
        }
        Value::Filesize { val, .. } => Value::Int { val: *val, span },
        Value::Float { val, .. } => Value::Int {
            val: {
                if radix == 10 {
                    *val as i64
                } else {
                    match convert_int(
                        &Value::Int {
                            val: *val as i64,
                            span,
                        },
                        span,
                        radix,
                    )
                    .as_i64()
                    {
                        Ok(v) => v,
                        _ => {
                            return Value::Error {
                                error: ShellError::CantConvert(
                                    "float".to_string(),
                                    "integer".to_string(),
                                    span,
                                    None,
                                ),
                            }
                        }
                    }
                }
            },
            span,
        },
        Value::String { val, .. } => {
            if radix == 10 {
                match int_from_string(val, span) {
                    Ok(val) => Value::Int { val, span },
                    Err(error) => Value::Error { error },
                }
            } else {
                convert_int(input, span, radix)
            }
        }
        Value::Bool { val, .. } => {
            if *val {
                Value::Int { val: 1, span }
            } else {
                Value::Int { val: 0, span }
            }
        }
        Value::Date { val, .. } => Value::Int {
            val: val.timestamp(),
            span,
        },
        Value::Duration { val, .. } => Value::Int { val: *val, span },
        Value::Binary { val, span } => {
            use byteorder::{BigEndian, ByteOrder, LittleEndian};

            let mut val = val.to_vec();

            if little_endian {
                while val.len() < 8 {
                    val.push(0);
                }
                val.resize(8, 0);

                Value::int(LittleEndian::read_i64(&val), *span)
            } else {
                while val.len() < 8 {
                    val.insert(0, 0);
                }
                val.resize(8, 0);

                Value::int(BigEndian::read_i64(&val), *span)
            }
        }
        // Propagate errors by explicitly matching them before the final case.
        Value::Error { .. } => input.clone(),
        other => Value::Error {
            error: ShellError::OnlySupportsThisInputType {
                exp_input_type: "integer, float, filesize, date, string, binary, duration or bool"
                    .into(),
                wrong_type: other.get_type().to_string(),
                dst_span: span,
                src_span: other.expect_span(),
            },
        },
    }
}

fn convert_int(input: &Value, head: Span, radix: u32) -> Value {
    let i = match input {
        Value::Int { val, .. } => val.to_string(),
        Value::String { val, .. } => {
            let val = val.trim();
            if val.starts_with("0x") // hex
                || val.starts_with("0b") // binary
                || val.starts_with("0o")
            // octal
            {
                match int_from_string(val, head) {
                    Ok(x) => return Value::int(x, head),
                    Err(e) => return Value::Error { error: e },
                }
            } else if val.starts_with("00") {
                // It's a padded string
                match i64::from_str_radix(val, radix) {
                    Ok(n) => return Value::int(n, head),
                    Err(e) => {
                        return Value::Error {
                            error: ShellError::CantConvert(
                                "string".to_string(),
                                "int".to_string(),
                                head,
                                Some(e.to_string()),
                            ),
                        }
                    }
                }
            }
            val.to_string()
        }
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
    match i64::from_str_radix(i.trim(), radix) {
        Ok(n) => Value::int(n, head),
        Err(_reason) => Value::Error {
            error: ShellError::CantConvert("string".to_string(), "int".to_string(), head, None),
        },
    }
}

fn int_from_string(a_string: &str, span: Span) -> Result<i64, ShellError> {
    let trimmed = a_string.trim();
    match trimmed {
        b if b.starts_with("0b") => {
            let num = match i64::from_str_radix(b.trim_start_matches("0b"), 2) {
                Ok(n) => n,
                Err(_reason) => {
                    return Err(ShellError::CantConvert(
                        "int".to_string(),
                        "string".to_string(),
                        span,
                        Some(r#"digits following "0b" can only be 0 or 1"#.to_string()),
                    ))
                }
            };
            Ok(num)
        }
        h if h.starts_with("0x") => {
            let num =
                match i64::from_str_radix(h.trim_start_matches("0x"), 16) {
                    Ok(n) => n,
                    Err(_reason) => return Err(ShellError::CantConvert(
                        "int".to_string(),
                        "string".to_string(),
                        span,
                        Some(
                            r#"hexadecimal digits following "0x" should be in 0-9, a-f, or A-F"#
                                .to_string(),
                        ),
                    )),
                };
            Ok(num)
        }
        o if o.starts_with("0o") => {
            let num = match i64::from_str_radix(o.trim_start_matches("0o"), 8) {
                Ok(n) => n,
                Err(_reason) => {
                    return Err(ShellError::CantConvert(
                        "int".to_string(),
                        "string".to_string(),
                        span,
                        Some(r#"octal digits following "0o" should be in 0-7"#.to_string()),
                    ))
                }
            };
            Ok(num)
        }
        _ => match trimmed.parse::<i64>() {
            Ok(n) => Ok(n),
            Err(_) => match a_string.parse::<f64>() {
                Ok(f) => Ok(f as i64),
                _ => Err(ShellError::CantConvert(
                    "int".to_string(),
                    "string".to_string(),
                    span,
                    Some(format!(
                        r#"string "{trimmed}" does not represent a valid integer"#
                    )),
                )),
            },
        },
    }
}

#[cfg(test)]
mod test {
    use super::Value;
    use super::*;
    use nu_protocol::Type::Error;

    #[test]
    fn test_examples() {
        use crate::test_examples;

        test_examples(SubCommand {})
    }

    #[test]
    fn turns_to_integer() {
        let word = Value::test_string("10");
        let expected = Value::test_int(10);

        let actual = action(
            &word,
            &Arguments {
                radix: 10,
                cell_paths: None,
                little_endian: false,
            },
            Span::test_data(),
        );
        assert_eq!(actual, expected);
    }

    #[test]
    fn turns_binary_to_integer() {
        let s = Value::test_string("0b101");
        let actual = action(
            &s,
            &Arguments {
                radix: 10,
                cell_paths: None,
                little_endian: false,
            },
            Span::test_data(),
        );
        assert_eq!(actual, Value::test_int(5));
    }

    #[test]
    fn turns_hex_to_integer() {
        let s = Value::test_string("0xFF");
        let actual = action(
            &s,
            &Arguments {
                radix: 16,
                cell_paths: None,
                little_endian: false,
            },
            Span::test_data(),
        );
        assert_eq!(actual, Value::test_int(255));
    }

    #[test]
    fn communicates_parsing_error_given_an_invalid_integerlike_string() {
        let integer_str = Value::test_string("36anra");

        let actual = action(
            &integer_str,
            &Arguments {
                radix: 10,
                cell_paths: None,
                little_endian: false,
            },
            Span::test_data(),
        );

        assert_eq!(actual.get_type(), Error)
    }
}
