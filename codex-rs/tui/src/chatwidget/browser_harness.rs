//! Browser Harness fork: /secrets and /browser integrations that shell out to
//! the `browser-harness` Python CLI (the same store the embedded agent reads
//! through its `secret()` / `totp()` helpers).

use std::io::Write;
use std::process::Stdio;

use super::*;
use crate::app_event::BrowserHarnessSecretsFlow;

/// Resolve the browser-harness CLI. `browser-harness tui` exports
/// BROWSER_HARNESS_CLI pointing at the workspace wrapper; standalone runs fall
/// back to `browser-harness` on PATH.
fn browser_harness_cli() -> String {
    std::env::var("BROWSER_HARNESS_CLI")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "browser-harness".to_string())
}

fn run_secrets_cli(
    tx: crate::app_event_sender::AppEventSender,
    args: Vec<String>,
    stdin_value: Option<String>,
    success_message: String,
) {
    std::thread::spawn(move || {
        let mut command = std::process::Command::new(browser_harness_cli());
        command
            .arg("secrets")
            .args(&args)
            .stdin(if stdin_value.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let spawned = command.spawn();
        let result = (|| {
            let mut child = spawned.map_err(|err| format!("failed to run browser-harness: {err}"))?;
            if let Some(value) = stdin_value
                && let Some(mut stdin) = child.stdin.take()
            {
                let _ = stdin.write_all(value.as_bytes());
                let _ = stdin.write_all(b"\n");
            }
            let output = child
                .wait_with_output()
                .map_err(|err| format!("browser-harness did not finish: {err}"))?;
            if output.status.success() {
                Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let stdout = String::from_utf8_lossy(&output.stdout);
                let detail = if stderr.trim().is_empty() { stdout } else { stderr };
                Err(detail.trim().lines().last().unwrap_or("unknown error").to_string())
            }
        })();
        match result {
            Ok(stdout) => tx.send(AppEvent::BrowserHarnessCliResult {
                message: success_message,
                hint: (!stdout.is_empty()).then_some(stdout),
                is_error: false,
            }),
            Err(err) => tx.send(AppEvent::BrowserHarnessCliResult {
                message: "browser-harness secrets command failed".to_string(),
                hint: Some(err),
                is_error: true,
            }),
        }
    });
}

impl ChatWidget {
    /// `/secrets` — manage website passwords and TOTP seeds stored in the
    /// browser-harness encrypted store.
    pub(crate) fn open_secrets_popup(&mut self) {
        let items = vec![
            SelectionItem {
                name: "Add password".to_string(),
                description: Some("store a website password for the agent".to_string()),
                actions: vec![Box::new(|tx| {
                    tx.send(AppEvent::BrowserHarnessSecretsPrompt {
                        flow: BrowserHarnessSecretsFlow::Password,
                        domain: None,
                        name: None,
                    });
                })],
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "Add 2FA (TOTP)".to_string(),
                description: Some("store a TOTP seed; the agent fills live codes".to_string()),
                actions: vec![Box::new(|tx| {
                    tx.send(AppEvent::BrowserHarnessSecretsPrompt {
                        flow: BrowserHarnessSecretsFlow::Totp,
                        domain: None,
                        name: None,
                    });
                })],
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "List stored credentials".to_string(),
                description: Some("names and domains only — values stay encrypted".to_string()),
                actions: vec![Box::new(|tx| {
                    run_secrets_cli(
                        tx.clone(),
                        vec!["list".to_string()],
                        /*stdin_value*/ None,
                        "Stored credentials".to_string(),
                    );
                })],
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "Remove a credential".to_string(),
                description: Some("delete a stored password or TOTP seed".to_string()),
                actions: vec![Box::new(|tx| {
                    tx.send(AppEvent::BrowserHarnessSecretsPrompt {
                        flow: BrowserHarnessSecretsFlow::Remove,
                        domain: None,
                        name: None,
                    });
                })],
                dismiss_on_select: true,
                ..Default::default()
            },
        ];
        self.bottom_pane.show_modal_selection_view(SelectionViewParams {
            title: Some("Secrets".to_string()),
            subtitle: Some(
                "Stored encrypted in browser-harness; the agent uses secret()/totp() without ever seeing values"
                    .to_string(),
            ),
            footer_hint: Some(standard_popup_hint_line()),
            items,
            ..Default::default()
        });
        self.request_redraw();
    }

    /// Advance the staged /secrets prompt chain: domain → name → value.
    pub(crate) fn open_secrets_prompt(
        &mut self,
        flow: BrowserHarnessSecretsFlow,
        domain: Option<String>,
        name: Option<String>,
    ) {
        let tx = self.app_event_tx.clone();
        let view = match (domain, name) {
            (None, _) => CustomPromptView::new(
                match flow {
                    BrowserHarnessSecretsFlow::Password => "Add password — domain".to_string(),
                    BrowserHarnessSecretsFlow::Totp => "Add 2FA (TOTP) — domain".to_string(),
                    BrowserHarnessSecretsFlow::Remove => "Remove credential — domain".to_string(),
                },
                "example.com".to_string(),
                String::new(),
                Some("Credentials are only usable while the page is on this domain".to_string()),
                Box::new(move |domain: String| {
                    tx.send(AppEvent::BrowserHarnessSecretsPrompt {
                        flow,
                        domain: Some(domain),
                        name: None,
                    });
                }),
            ),
            (Some(domain), None) => CustomPromptView::new(
                "Placeholder name".to_string(),
                "e.g. login-password or github-2fa".to_string(),
                String::new(),
                Some(format!("{domain} — the agent sees this name, never the value")),
                Box::new(move |name: String| {
                    tx.send(AppEvent::BrowserHarnessSecretsPrompt {
                        flow,
                        domain: Some(domain.clone()),
                        name: Some(name),
                    });
                }),
            ),
            (Some(domain), Some(name)) => match flow {
                BrowserHarnessSecretsFlow::Remove => {
                    run_secrets_cli(
                        tx,
                        vec![
                            "remove".to_string(),
                            "--domain".to_string(),
                            domain.clone(),
                            "--name".to_string(),
                            name.clone(),
                        ],
                        /*stdin_value*/ None,
                        format!("Removed {name} for {domain}"),
                    );
                    return;
                }
                BrowserHarnessSecretsFlow::Password | BrowserHarnessSecretsFlow::Totp => {
                    let is_totp = flow == BrowserHarnessSecretsFlow::Totp;
                    CustomPromptView::new(
                        if is_totp {
                            "TOTP seed (base32)".to_string()
                        } else {
                            "Password".to_string()
                        },
                        if is_totp {
                            "paste the base32 setup key from the website".to_string()
                        } else {
                            "typed value stays local, encrypted at rest".to_string()
                        },
                        String::new(),
                        Some(format!("{domain} · {name}")),
                        Box::new(move |value: String| {
                            let mut args = vec![
                                "set".to_string(),
                                "--domain".to_string(),
                                domain.clone(),
                                "--name".to_string(),
                                name.clone(),
                                "--stdin".to_string(),
                            ];
                            if is_totp {
                                args.push("--totp".to_string());
                            }
                            run_secrets_cli(
                                tx.clone(),
                                args,
                                Some(value),
                                format!(
                                    "Stored {kind} {name} for {domain}",
                                    kind = if is_totp { "2FA seed" } else { "password" },
                                ),
                            );
                        }),
                    )
                    .masked()
                }
            },
        };
        self.bottom_pane.show_view(Box::new(view));
        self.request_redraw();
    }

    /// Transcript feedback for finished browser-harness CLI calls.
    pub(crate) fn on_browser_harness_cli_result(
        &mut self,
        message: String,
        hint: Option<String>,
        is_error: bool,
    ) {
        if is_error {
            let detail = hint
                .map(|hint| format!("{message}: {hint}"))
                .unwrap_or(message);
            self.add_error_message(detail);
        } else {
            self.add_info_message(message, hint);
        }
    }

    /// `/browser` — choose the browser backend for the harness workspace.
    pub(crate) fn open_browser_backend_popup(&mut self) {
        let Some(agent_root) = std::env::var("BROWSER_HARNESS_AGENT_ROOT")
            .ok()
            .filter(|root| !root.trim().is_empty())
        else {
            self.add_info_message(
                "Browser backend selection needs a Browser Harness workspace.".to_string(),
                Some("Start this TUI through `browser-harness tui`.".to_string()),
            );
            return;
        };
        let make_item = |name: &str,
                         description: &str,
                         env_lines: &'static [&'static str],
                         label: &'static str| {
            let root = agent_root.clone();
            SelectionItem {
                name: name.to_string(),
                description: Some(description.to_string()),
                actions: vec![Box::new(move |tx| {
                    let result = write_browser_env(&root, env_lines, label);
                    match result {
                        Ok(()) => tx.send(AppEvent::BrowserHarnessCliResult {
                            message: format!("Browser backend set to {label}"),
                            hint: Some("applies to the agent's next browser call".to_string()),
                            is_error: false,
                        }),
                        Err(err) => tx.send(AppEvent::BrowserHarnessCliResult {
                            message: "Failed to update browser backend".to_string(),
                            hint: Some(err.to_string()),
                            is_error: true,
                        }),
                    }
                })],
                dismiss_on_select: true,
                ..Default::default()
            }
        };
        self.bottom_pane.show_modal_selection_view(SelectionViewParams {
            title: Some("Browser".to_string()),
            subtitle: Some("Choose the browser backend for this workspace".to_string()),
            footer_hint: Some(standard_popup_hint_line()),
            items: vec![
                make_item(
                    "Local Chrome",
                    "connect to the Chrome running on this machine",
                    &["unset BU_CDP_WS BU_CDP_URL BU_AUTOSPAWN 2>/dev/null || true"],
                    "Local Chrome",
                ),
                make_item(
                    "Browser Use Cloud",
                    "provision a cloud browser (needs browser-harness auth login)",
                    &[
                        "unset BU_CDP_WS BU_CDP_URL 2>/dev/null || true",
                        "export BU_AUTOSPAWN=1",
                    ],
                    "Browser Use Cloud",
                ),
            ],
            ..Default::default()
        });
        self.request_redraw();
    }
}

/// Persist backend env exports where the workspace `bin/browser-harness`
/// wrapper sources them, and mirror the label into this process so the
/// composer's bottom-border tag updates immediately.
fn write_browser_env(
    agent_root: &str,
    env_lines: &[&str],
    label: &str,
) -> std::io::Result<()> {
    let path = std::path::Path::new(agent_root).join("browser-env");
    let mut content = String::from("# generated by /browser in the Browser Harness TUI\n");
    for line in env_lines {
        content.push_str(line);
        content.push('\n');
    }
    std::fs::write(&path, content)?;
    // Rendering reads BH_BROWSER_LABEL every frame; single-threaded TUI event
    // loop makes this safe in practice.
    unsafe {
        std::env::set_var("BH_BROWSER_LABEL", label);
    }
    Ok(())
}
