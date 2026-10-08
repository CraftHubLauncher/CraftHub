//! Headless front-end over the CraftHub engine, sharing the GUI's database and folders.
//! Intended for smoke tests and scripting; do not run it while the GUI is modifying apps.

use std::io::Write;
use std::sync::Arc;

use crafthub_core::engine::{AppStatus, Phase, ProgressEvent};
use crafthub_core::{CoreError, Engine, default_data_dir, production_config};

const USAGE: &str = "crafthub-cli <command>

Commands:
  list                    Show catalog with install/update status (cached data)
  check [app]             Query GitHub for releases, then list
  versions <app>          Show all releases and whether they are installable
  install <app> [version] Install newest (or a specific) version
  update <app>            Update to the newest version in the current channel
  update-all              Update every installed app sequentially
  rollback <app>          Switch back to the retained previous version
  uninstall <app>         Remove CraftHub-managed files for the app
  launch <app>            Start the installed app
  history                 Recent install/update events
  paths                   Show managed folders";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("runtime");
    match rt.block_on(run(args)) {
        Ok(()) => {}
        Err(e) => {
            eprintln!("error [{}]: {e}", e.kind());
            std::process::exit(1);
        }
    }
}

fn progress_printer() -> crafthub_core::ProgressSink {
    Arc::new(|e: ProgressEvent| {
        let mut out = std::io::stderr();
        match e.phase {
            Phase::Downloading if e.total > 0 => {
                let _ = write!(
                    out,
                    "\r  downloading {:>5.1}% ({} / {} bytes)",
                    e.done as f64 * 100.0 / e.total as f64,
                    e.done,
                    e.total
                );
                if e.done == e.total {
                    let _ = writeln!(out);
                }
            }
            Phase::Extracting if e.total > 0 && e.done == e.total => {
                let _ = writeln!(out, "  extracted {} bytes", e.total);
            }
            Phase::Downloading | Phase::Extracting => {}
            p => {
                let _ = writeln!(
                    out,
                    "  [{p:?}]{}",
                    e.message.map(|m| format!(" {m}")).unwrap_or_default()
                );
            }
        }
    })
}

fn arg(args: &[String], i: usize) -> Result<&str, CoreError> {
    args.get(i)
        .map(String::as_str)
        .ok_or_else(|| CoreError::InvalidInput(format!("missing argument\n\n{USAGE}")))
}

async fn run(args: Vec<String>) -> Result<(), CoreError> {
    let Some(cmd) = args.first().map(String::as_str) else {
        println!("{USAGE}");
        return Ok(());
    };
    let data = default_data_dir().ok_or_else(|| CoreError::Unsupported("no data folder".into()))?;
    let engine = Engine::new(production_config(&data)?)?;
    match cmd {
        "list" => print_apps(&engine.list_apps()?),
        "check" => print_apps(
            &engine
                .check_for_updates(args.get(1).map(String::as_str))
                .await?,
        ),
        "versions" => {
            for v in engine.versions(arg(&args, 1)?).await? {
                println!(
                    "{:<16} {:<10} {:<12} {}",
                    v.tag,
                    if v.prerelease { "pre" } else { "stable" },
                    if v.installable {
                        "installable"
                    } else {
                        "unavailable"
                    },
                    v.asset
                        .as_ref()
                        .map(|a| a.name.as_str())
                        .or(v.unavailable_reason.as_deref())
                        .unwrap_or("")
                );
            }
        }
        "install" => {
            let o = engine
                .install(
                    arg(&args, 1)?,
                    args.get(2).map(String::as_str),
                    progress_printer(),
                )
                .await?;
            println!(
                "installed {} {} -> {}\n  verification: {}",
                o.app_id, o.version, o.path, o.verification
            );
            for w in o.warnings {
                println!("  warning: {w}");
            }
        }
        "update" => {
            let o = engine.update(arg(&args, 1)?, progress_printer()).await?;
            println!(
                "updated {} {} -> {}\n  verification: {}",
                o.app_id,
                o.previous_version.unwrap_or_default(),
                o.version,
                o.verification
            );
            for w in o.warnings {
                println!("  warning: {w}");
            }
        }
        "update-all" => {
            let s = engine.update_all(progress_printer()).await?;
            println!("{}", serde_json::to_string_pretty(&s).unwrap_or_default());
        }
        "rollback" => {
            let v = engine.rollback(arg(&args, 1)?)?;
            println!(
                "now on {} (previous: {})",
                v.version,
                v.previous_version.unwrap_or_default()
            );
        }
        "uninstall" => {
            let o = engine.uninstall(arg(&args, 1)?)?;
            println!("removed {} files", o.removed_files);
            if let Some(p) = o.kept_path {
                println!("kept {:?} in {p}", o.kept_entries);
            }
        }
        "launch" => println!("started pid {}", engine.launch(arg(&args, 1)?)?),
        "history" => {
            for e in engine.events(50)? {
                println!(
                    "{} {:<12} {:<10} {:<10} {:<12} {}",
                    e.at,
                    e.app_id,
                    e.kind,
                    e.version.unwrap_or_default(),
                    e.outcome,
                    e.message.unwrap_or_default()
                );
            }
        }
        "paths" => {
            let p = engine.paths();
            println!(
                "apps:      {}\nstaging:   {}\ndownloads: {}\ndata:      {}",
                p.apps.display(),
                p.staging.display(),
                p.downloads.display(),
                data.display()
            );
        }
        _ => println!("{USAGE}"),
    }
    Ok(())
}

fn print_apps(apps: &[crafthub_core::AppView]) {
    for a in apps {
        let status = match a.status {
            AppStatus::Unavailable => "unavailable",
            AppStatus::NotInstalled => "not installed",
            AppStatus::Installed => "installed",
            AppStatus::UpdateAvailable => "update available",
        };
        println!(
            "{:<12} {:<17} installed={:<10} latest={:<10}{}{}",
            a.id,
            status,
            a.installed
                .as_ref()
                .map(|i| i.version.as_str())
                .unwrap_or("-"),
            a.latest.as_ref().map(|l| l.version.as_str()).unwrap_or("-"),
            if a.running { " [running]" } else { "" },
            a.unsupported_reason
                .as_ref()
                .map(|r| format!("  ({r})"))
                .unwrap_or_default()
        );
        if let Some(w) = a.check.warning.as_ref().or(a.check.error.as_ref()) {
            println!("{:<12} ! {w}", "");
        }
    }
}
