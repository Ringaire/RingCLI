use std::io::{self, BufRead, Write};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SdkInput {
    pub id: Uuid,
    #[serde(rename = "type")]
    pub msg_type: String,
    pub payload: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SdkOutput {
    pub id: Uuid,
    #[serde(rename = "type")]
    pub msg_type: String,
    pub payload: String,
}

/// Run SDK mode: read JSON lines from stdin, write responses to stdout.
pub async fn run(_output_format: &str) -> Result<()> {
    let stdin = io::stdin();
    let reader = stdin.lock();
    let mut line = String::new();

    // Write ready signal
    writeln!(io::stdout(), "{}", serde_json::to_string(&SdkOutput {
        id: Uuid::nil(),
        msg_type: "ready".to_string(),
        payload: "Neko SDK ready".to_string(),
    })?)?;
    io::stdout().flush()?;

    for raw in reader.lines() {
        line.clear();
        match raw {
            Ok(text) if text.trim().is_empty() => continue,
            Ok(text) => {
                match serde_json::from_str::<SdkInput>(&text) {
                    Ok(input) => {
                        match input.msg_type.as_str() {
                            "message" => {
                                // Echo back — actual agent processing will go here
                                let out = SdkOutput {
                                    id: input.id,
                                    msg_type: "text".to_string(),
                                    payload: input.payload,
                                };
                                writeln!(io::stdout(), "{}", serde_json::to_string(&out)?)?;
                                io::stdout().flush()?;

                                let done = SdkOutput {
                                    id: input.id,
                                    msg_type: "done".to_string(),
                                    payload: String::new(),
                                };
                                writeln!(io::stdout(), "{}", serde_json::to_string(&done)?)?;
                                io::stdout().flush()?;
                            }
                            "ping" => {
                                let out = SdkOutput {
                                    id: input.id,
                                    msg_type: "pong".to_string(),
                                    payload: String::new(),
                                };
                                writeln!(io::stdout(), "{}", serde_json::to_string(&out)?)?;
                                io::stdout().flush()?;
                            }
                            "exit" | "stop" => break,
                            other => {
                                let err = SdkOutput {
                                    id: input.id,
                                    msg_type: "error".to_string(),
                                    payload: format!("unknown type: {other}"),
                                };
                                writeln!(io::stdout(), "{}", serde_json::to_string(&err)?)?;
                                io::stdout().flush()?;
                            }
                        }
                    }
                    Err(e) => {
                        let err = SdkOutput {
                            id: Uuid::nil(),
                            msg_type: "error".to_string(),
                            payload: format!("parse error: {e}"),
                        };
                        writeln!(io::stdout(), "{}", serde_json::to_string(&err)?)?;
                        io::stdout().flush()?;
                    }
                }
            }
            Err(e) => {
                let err = SdkOutput {
                    id: Uuid::nil(),
                    msg_type: "error".to_string(),
                    payload: format!("stdin error: {e}"),
                };
                let _ = writeln!(io::stdout(), "{}", serde_json::to_string(&err)?);
                break;
            }
        }
    }

    Ok(())
}
