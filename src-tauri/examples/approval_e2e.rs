//! Real-CLI check of "ask before changes": `cargo run --example approval_e2e -- <empty git dir>`.
use nekotrail_lib::agent;
use std::io::{BufRead, BufReader, Write};
fn main() -> anyhow::Result<()> {
    let dir = std::env::args().nth(1).unwrap();
    let spec = agent::TurnSpec { fork: false, agent: "claude", prompt: "Create a file named note.txt containing the single word purr, using the Write tool. Nothing else.", agent_session_id: None, model: Some("haiku"), permission_mode: "ask", cwd: std::path::Path::new(&dir), effort: None, instructions: None, env: &[], dirs: &[], project_config: true, guard: None };
    let mut child = agent::command(&spec).stdout(std::process::Stdio::piped()).spawn()?;
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(agent::user_message(spec.prompt).as_bytes())?;
    let mut stdin = Some(stdin);
    for line in BufReader::new(child.stdout.take().unwrap()).lines() {
        let line = line?;
        let Ok(j) = serde_json::from_str::<serde_json::Value>(&line) else { continue };
        if j["type"] == "control_request" {
            println!("asked: {} {}", j["request"]["tool_name"], agent::summarize_input(j["request"]["tool_name"].as_str().unwrap_or(""), &j["request"]["input"]));
            let reply = agent::approval_response(j["request_id"].as_str().unwrap(), true, &j["request"]["input"], &j["request"]["permission_suggestions"], false);
            stdin.as_mut().unwrap().write_all(reply.as_bytes())?;
            stdin.as_mut().unwrap().flush()?;
        }
        if j["type"] == "result" { println!("result: {} cost ${:.4}", j["result"], j["total_cost_usd"].as_f64().unwrap_or(0.0)); stdin = None; }
    }
    drop(stdin);
    child.wait()?;
    println!("file: {:?}", std::fs::read_to_string(format!("{dir}/note.txt")).ok());
    Ok(())
}
