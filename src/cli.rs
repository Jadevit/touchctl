use anyhow::{Result, anyhow};
use pico_args::Arguments;
use std::{env, process::Command};

use crate::ipc;

pub fn run() -> Result<()> {
    let mut pargs = Arguments::from_env();

    // Hidden daemon mode (spawned by `start`)
    if pargs.contains("--daemon") {
        return ipc::run_daemon();
    }

    // No args -> general help
    if env::args().len() == 1 {
        print_help();
        return Ok(());
    }

    // Flags-based help (-h/--help)
    if pargs.contains("-h") || pargs.contains("--help") {
        print_help();
        return Ok(());
    }

    // First free arg is the subcommand
    let subcmd: Option<String> = pargs.free_from_str().ok();

    match subcmd.as_deref() {
        Some("help") => {
            let topic: Option<String> = pargs.free_from_str().ok();
            if let Some(t) = topic {
                print_subcmd_help(&t);
            } else {
                print_help();
            }
            Ok(())
        }

        Some("start") => {
            let exe = std::env::current_exe()?;
            let mut child = Command::new(exe).arg("--daemon").spawn()?;
            println!("touchctl: started daemon (pid={})", child.id());
            Ok(())
        }

        Some("stop") => {
            let r = ipc::client_request(serde_json::json!({"op":"shutdown"}))?;
            print_response(&r);
            Ok(())
        }

        Some("status") => {
            let r = ipc::client_request(serde_json::json!({"op":"status"}))?;
            print_response(&r);
            Ok(())
        }

        Some("reload") => {
            let r = ipc::client_request(serde_json::json!({"op":"reload"}))?;
            print_response(&r);
            Ok(())
        }

        Some("use") => {
            let name: String = pargs
                .free_from_str()
                .map_err(|_| anyhow!("usage: touchctl use <profile_name>"))?;
            let r = ipc::client_request(serde_json::json!({"op":"use","profile":name}))?;
            print_response(&r);
            Ok(())
        }

        Some("list") => {
            let r = ipc::client_request(serde_json::json!({"op":"list"}))?;
            print_response(&r);
            Ok(())
        }

        Some("doctor") => {
            let r = ipc::client_request(serde_json::json!({"op":"doctor"}))?;
            print_response(&r);
            Ok(())
        }

        Some("emit") => {
            // usage:
            //   touchctl emit click right
            //   touchctl emit scroll 3
            //   touchctl emit key CTRL+EQUAL
            let what: String = pargs
                .free_from_str()
                .map_err(|_| anyhow!("usage: touchctl emit <click|scroll|key> ..."))?;
            let mut sink = crate::actions::UinputSink::new()?;
            match what.as_str() {
                "click" => {
                    let btn: String = pargs
                        .free_from_str()
                        .map_err(|_| anyhow!("usage: touchctl emit click <left|right|middle>"))?;
                    sink.click_mouse(&btn)?;
                    println!("ok: clicked {btn}");
                }
                "scroll" => {
                    let steps: i32 = pargs
                        .free_from_str()
                        .map_err(|_| anyhow!("usage: touchctl emit scroll <steps>"))?;
                    sink.scroll_vertical(steps)?;
                    println!("ok: scrolled vertical {steps}");
                }
                "key" => {
                    let chord: String = pargs
                        .free_from_str()
                        .map_err(|_| anyhow!("usage: touchctl emit key CTRL+EQUAL"))?;
                    sink.key_chord(&chord)?;
                    println!("ok: sent key chord {chord}");
                }
                other => return Err(anyhow!("unknown emit kind: {other}")),
            }
            Ok(())
        }

        Some(other) => {
            eprintln!("unknown subcommand: {other}\n");
            print_help();
            Ok(())
        }

        None => {
            print_help();
            Ok(())
        }
    }
}

fn print_help() {
    println!(
        r#"touchctl — Linux gesture daemon (skeleton)

USAGE:
  touchctl help [command]                 Show general or command-specific help
  touchctl start                          Start the daemon
  touchctl stop                           Stop the daemon
  touchctl status                         Show daemon state
  touchctl reload                         Reload active profile
  touchctl use <name>                     Switch active profile
  touchctl list                           List profiles
  touchctl doctor                         Diagnose permissions/devices
  touchctl emit click <left|right|middle> Emit a mouse click
  touchctl emit scroll <steps>            Emit vertical scroll (+/- steps)
  touchctl emit key CTRL+EQUAL            Emit a key or chord

TIPS:
  - Install systemd user unit: ~/.config/systemd/user/touchctl.service
  - Profiles: ~/.config/touchctl/profiles
  - Active profile pointer: ~/.config/touchctl/active
"#
    );
}

fn print_subcmd_help(cmd: &str) {
    match cmd {
        "start" => println!("usage: touchctl start\nStarts the background daemon."),
        "stop" => println!("usage: touchctl stop\nStops the running daemon."),
        "status" => println!(
            "usage: touchctl status\nShows enabled flag, active profile, devices, socket, PID."
        ),
        "reload" => println!(
            "usage: touchctl reload\nReloads the current profile; keeps last good on error."
        ),
        "use" => {
            println!("usage: touchctl use <name>\nSwitches active profile to <name> and reloads.")
        }
        "list" => {
            println!("usage: touchctl list\nLists available profiles; marks active with '*'.")
        }
        "doctor" => println!(
            "usage: touchctl doctor\nChecks permissions and lists detected multitouch devices."
        ),
        "emit" => println!(
            "usage:\n  touchctl emit click <left|right|middle>\n  touchctl emit scroll <steps>\n  touchctl emit key CTRL+EQUAL"
        ),
        _ => {
            eprintln!("unknown command: {cmd}\n");
            print_help();
        }
    }
}

fn print_response(v: &serde_json::Value) {
    println!("{}", serde_json::to_string_pretty(v).unwrap_or_default());
}
