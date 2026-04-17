//! Binary OTLP (`.pb` / `.otlp`) parser — reads protobuf-encoded
//! OpenTelemetry trace exports.
//!
//! Gated behind the `otel-proto` Cargo feature because `prost` and its
//! transitive deps add meaningful binary size. Users who only consume JSON
//! traces don't pay that cost; users who do need protobuf rebuild with
//! `--features otel-proto`.
//!
//! When the feature is off, `load` returns a helpful error rather than a
//! confusing UTF-8 decode failure, so the failure mode is "agx tells you
//! how to fix it" not "panic".
//!
//! The protobuf-decoding path reuses `otel_json::append_span` for the
//! actual span → Step conversion. Only the wire decode is different.

// Imports used only by the stub path below. The feature-on path lives in
// `real` and has its own imports — keeping top-level imports minimal avoids
// unused-import warnings when `otel-proto` is enabled.
#[cfg(not(feature = "otel-proto"))]
use {crate::timeline::Step, anyhow::Result, std::path::Path};

/// Entry point for loading a binary OTLP file. When the `otel-proto` feature
/// is disabled, returns a helpful error pointing the user at the rebuild
/// instructions.
#[cfg(not(feature = "otel-proto"))]
pub fn load(_path: &Path) -> Result<Vec<Step>> {
    anyhow::bail!(
        "binary OTLP (.pb / .otlp) support requires rebuilding agx with --features otel-proto.\n\
         \tInstall: cargo install agx --features otel-proto\n\
         \tBuild:   cargo build --release --features otel-proto"
    )
}

#[cfg(feature = "otel-proto")]
pub use real::load;

#[cfg(feature = "otel-proto")]
mod real {
    use crate::otel_json::append_span;
    use crate::timeline::{Step, compute_durations};
    use anyhow::{Context, Result};
    use prost::Message;
    use std::collections::HashMap;
    use std::path::Path;

    // Minimal subset of the OTLP trace protobuf schema — only fields agx
    // actually reads. Unknown fields decode without error and are ignored,
    // so we're resilient to schema growth. Tags match the canonical OTLP
    // proto file (trace.proto v1).

    // prost's `Message` derive emits a `Default` impl (protobuf-3 zero
    // defaults), so deriving both `Default` and `Message` conflicts. We
    // rely on the prost-generated Default below for every struct.

    #[derive(Clone, PartialEq, Message)]
    struct TracesData {
        #[prost(message, repeated, tag = "1")]
        resource_spans: Vec<ResourceSpans>,
    }

    #[derive(Clone, PartialEq, Message)]
    struct ResourceSpans {
        #[prost(message, repeated, tag = "2")]
        scope_spans: Vec<ScopeSpans>,
    }

    #[derive(Clone, PartialEq, Message)]
    struct ScopeSpans {
        #[prost(message, repeated, tag = "2")]
        spans: Vec<Span>,
    }

    #[derive(Clone, PartialEq, Message)]
    struct Span {
        #[prost(fixed64, tag = "7")]
        start_time_unix_nano: u64,
        #[prost(message, repeated, tag = "9")]
        attributes: Vec<KeyValue>,
    }

    #[derive(Clone, PartialEq, Message)]
    struct KeyValue {
        #[prost(string, tag = "1")]
        key: String,
        #[prost(message, optional, tag = "2")]
        value: Option<AnyValue>,
    }

    #[derive(Clone, PartialEq, Message)]
    struct AnyValue {
        #[prost(oneof = "any_value::Value", tags = "1, 2, 3, 4")]
        value: Option<any_value::Value>,
    }

    pub mod any_value {
        // Variant names mirror the OTLP `AnyValue` protobuf oneof field
        // names (string_value / bool_value / int_value / double_value) —
        // renaming would diverge from the spec and make cross-referencing
        // harder. Suppress clippy::enum_variant_names accordingly.
        #[derive(Clone, PartialEq, ::prost::Oneof)]
        #[allow(clippy::enum_variant_names)]
        pub enum Value {
            #[prost(string, tag = "1")]
            StringValue(String),
            #[prost(bool, tag = "2")]
            BoolValue(bool),
            #[prost(int64, tag = "3")]
            IntValue(i64),
            #[prost(double, tag = "4")]
            DoubleValue(f64),
        }
    }

    pub fn load(path: &Path) -> Result<Vec<Step>> {
        let bytes = std::fs::read(path)
            .with_context(|| format!("reading binary OTLP file: {}", path.display()))?;
        let data = TracesData::decode(bytes.as_slice())
            .with_context(|| format!("decoding OTLP protobuf: {}", path.display()))?;

        // Flatten every span into a single chronologically-ordered list.
        // Same convention as otel_json::load so multi-resource / multi-scope
        // files produce one coherent timeline.
        let mut all_spans: Vec<(&Span, u64)> = Vec::new();
        for rs in &data.resource_spans {
            for ss in &rs.scope_spans {
                for span in &ss.spans {
                    all_spans.push((span, span.start_time_unix_nano));
                }
            }
        }
        all_spans.sort_by_key(|(_, ts)| *ts);

        let mut steps: Vec<Step> = Vec::new();
        for (span, ts_ns) in all_spans {
            // Convert prost attrs → owned HashMap<String, Value>, then
            // re-borrow as HashMap<&str, Value> to match the signature
            // append_span already uses for the JSON path.
            let attrs_owned = index_attributes(&span.attributes);
            let attrs_borrowed: HashMap<&str, serde_json::Value> = attrs_owned
                .iter()
                .map(|(k, v)| (k.as_str(), v.clone()))
                .collect();
            append_span(&attrs_borrowed, ts_ns, &mut steps);
        }
        compute_durations(&mut steps);
        Ok(steps)
    }

    fn index_attributes(attrs: &[KeyValue]) -> HashMap<String, serde_json::Value> {
        let mut out = HashMap::new();
        for kv in attrs {
            let Some(v) = kv.value.as_ref() else {
                continue;
            };
            let Some(jv) = any_value_to_json(v) else {
                continue;
            };
            out.insert(kv.key.clone(), jv);
        }
        out
    }

    fn any_value_to_json(v: &AnyValue) -> Option<serde_json::Value> {
        let inner = v.value.as_ref()?;
        match inner {
            any_value::Value::StringValue(s) => Some(serde_json::Value::String(s.clone())),
            any_value::Value::BoolValue(b) => Some(serde_json::Value::Bool(*b)),
            // OTel usage counters are non-negative; drop negative ints
            // rather than silently mapping to a wrong u64.
            any_value::Value::IntValue(i) if *i >= 0 =>
            {
                #[allow(clippy::cast_sign_loss)]
                Some(serde_json::Value::Number((*i as u64).into()))
            }
            any_value::Value::IntValue(_) => None,
            any_value::Value::DoubleValue(d) => {
                serde_json::Number::from_f64(*d).map(serde_json::Value::Number)
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::timeline::StepKind;
        use std::io::Write;
        use tempfile::NamedTempFile;

        fn write_bytes(bytes: &[u8]) -> NamedTempFile {
            let mut f = NamedTempFile::new().unwrap();
            f.write_all(bytes).unwrap();
            f
        }

        fn kv_str(k: &str, v: &str) -> KeyValue {
            KeyValue {
                key: k.into(),
                value: Some(AnyValue {
                    value: Some(any_value::Value::StringValue(v.into())),
                }),
            }
        }

        fn kv_int(k: &str, v: i64) -> KeyValue {
            KeyValue {
                key: k.into(),
                value: Some(AnyValue {
                    value: Some(any_value::Value::IntValue(v)),
                }),
            }
        }

        fn encode(data: &TracesData) -> Vec<u8> {
            data.encode_to_vec()
        }

        fn minimal_chat_bytes() -> Vec<u8> {
            encode(&TracesData {
                resource_spans: vec![ResourceSpans {
                    scope_spans: vec![ScopeSpans {
                        spans: vec![Span {
                            start_time_unix_nano: 1_000_000_000,
                            attributes: vec![
                                kv_str("gen_ai.operation.name", "chat"),
                                kv_str("gen_ai.request.model", "gpt-5"),
                                kv_int("gen_ai.usage.input_tokens", 100),
                                kv_int("gen_ai.usage.output_tokens", 50),
                                kv_str("gen_ai.prompt.0.role", "user"),
                                kv_str("gen_ai.prompt.0.content", "hello"),
                                kv_str("gen_ai.completion.0.role", "assistant"),
                                kv_str("gen_ai.completion.0.content", "hi"),
                            ],
                        }],
                    }],
                }],
            })
        }

        #[test]
        fn decodes_minimal_chat_span_into_two_steps() {
            let f = write_bytes(&minimal_chat_bytes());
            let steps = load(f.path()).unwrap();
            assert_eq!(steps.len(), 2);
            assert_eq!(steps[0].kind, StepKind::UserText);
            assert_eq!(steps[1].kind, StepKind::AssistantText);
            assert!(steps[0].detail.contains("hello"));
            assert!(steps[1].detail.contains("hi"));
        }

        #[test]
        fn usage_and_model_attach_to_first_step() {
            let f = write_bytes(&minimal_chat_bytes());
            let steps = load(f.path()).unwrap();
            assert_eq!(steps[0].model.as_deref(), Some("gpt-5"));
            assert_eq!(steps[0].tokens_in, Some(100));
            assert_eq!(steps[0].tokens_out, Some(50));
            assert_eq!(steps[1].model, None);
        }

        #[test]
        fn execute_tool_span_produces_paired_use_and_result() {
            let data = TracesData {
                resource_spans: vec![ResourceSpans {
                    scope_spans: vec![ScopeSpans {
                        spans: vec![Span {
                            start_time_unix_nano: 2_000_000_000,
                            attributes: vec![
                                kv_str("gen_ai.operation.name", "execute_tool"),
                                kv_str("gen_ai.tool.name", "list_dir"),
                                kv_str("gen_ai.tool.call.id", "call_x"),
                                kv_str("gen_ai.tool.call.arguments", r#"{"p":"."}"#),
                                kv_str("gen_ai.tool.call.result", "a\nb\n"),
                            ],
                        }],
                    }],
                }],
            };
            let f = write_bytes(&encode(&data));
            let steps = load(f.path()).unwrap();
            assert_eq!(steps.len(), 2);
            assert_eq!(steps[0].kind, StepKind::ToolUse);
            assert!(steps[0].label.contains("list_dir"));
            assert_eq!(steps[1].kind, StepKind::ToolResult);
            assert!(steps[1].detail.contains("a\nb"));
        }

        #[test]
        fn spans_sorted_by_start_time_across_resource_boundaries() {
            let data = TracesData {
                resource_spans: vec![
                    ResourceSpans {
                        scope_spans: vec![ScopeSpans {
                            spans: vec![Span {
                                start_time_unix_nano: 3_000_000_000,
                                attributes: vec![
                                    kv_str("gen_ai.operation.name", "chat"),
                                    kv_str("gen_ai.prompt.0.role", "user"),
                                    kv_str("gen_ai.prompt.0.content", "third"),
                                ],
                            }],
                        }],
                    },
                    ResourceSpans {
                        scope_spans: vec![ScopeSpans {
                            spans: vec![Span {
                                start_time_unix_nano: 1_000_000_000,
                                attributes: vec![
                                    kv_str("gen_ai.operation.name", "chat"),
                                    kv_str("gen_ai.prompt.0.role", "user"),
                                    kv_str("gen_ai.prompt.0.content", "first"),
                                ],
                            }],
                        }],
                    },
                ],
            };
            let f = write_bytes(&encode(&data));
            let steps = load(f.path()).unwrap();
            assert_eq!(steps.len(), 2);
            assert!(steps[0].detail.contains("first"));
            assert!(steps[1].detail.contains("third"));
        }

        #[test]
        fn spans_without_genai_marker_are_ignored() {
            let data = TracesData {
                resource_spans: vec![ResourceSpans {
                    scope_spans: vec![ScopeSpans {
                        spans: vec![
                            // Non-GenAI span — no gen_ai.operation.name.
                            Span {
                                start_time_unix_nano: 1_000_000_000,
                                attributes: vec![kv_str("http.method", "GET")],
                            },
                            // GenAI chat span.
                            Span {
                                start_time_unix_nano: 2_000_000_000,
                                attributes: vec![
                                    kv_str("gen_ai.operation.name", "chat"),
                                    kv_str("gen_ai.prompt.0.role", "user"),
                                    kv_str("gen_ai.prompt.0.content", "hi"),
                                ],
                            },
                        ],
                    }],
                }],
            };
            let f = write_bytes(&encode(&data));
            let steps = load(f.path()).unwrap();
            assert_eq!(steps.len(), 1);
            assert_eq!(steps[0].kind, StepKind::UserText);
        }

        #[test]
        fn invalid_protobuf_returns_error() {
            // Not valid protobuf — decode should fail with a contextual error.
            let f = write_bytes(&[0xff, 0xfe, 0xfd, 0xfc]);
            let err = load(f.path()).unwrap_err();
            let msg = format!("{err:#}");
            assert!(
                msg.contains("decoding OTLP protobuf"),
                "expected decode error context, got: {msg}"
            );
        }
    }
}

// Stub-path tests — these run when the feature is OFF (the default CI
// build). Verify the helpful error message shows up.
#[cfg(not(feature = "otel-proto"))]
#[cfg(test)]
mod stub_tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn load_returns_helpful_error_when_feature_disabled() {
        let err = load(&PathBuf::from("/dev/null")).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("--features otel-proto"));
        assert!(msg.contains("cargo install agx"));
    }
}
