use crate::llm_client::LlmClient;
use crate::prompt;
use crate::types::{CommandHelp, FlatCommand};
use anyhow::{Result, anyhow};
use log::debug;
use std::future::Future;
use std::pin::Pin;
use tokio::process::Command;

#[derive(Debug)]
struct CommandOutput {
    stdout: String,
    stderr: String,
}

async fn execute_command(cmd: &str, args: &[String]) -> Result<CommandOutput> {
    let output = Command::new(cmd)
        .args(args)
        .output()
        .await
        .map_err(|e| anyhow!("Failed to spawn command {}: {}", cmd, e))?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    Ok(CommandOutput { stdout, stderr })
}

const MAX_DEPTH: usize = 2;

pub struct HelpCrawler<'a> {
    llm_client: &'a LlmClient,
}

impl<'a> HelpCrawler<'a> {
    pub fn new(llm_client: &'a LlmClient) -> Self {
        Self { llm_client }
    }

    pub async fn crawl(&mut self, command: &str) -> Result<CommandHelp> {
        self.crawl_recursive(&[command.to_string()], 0).await
    }

    fn crawl_recursive<'b>(
        &'b mut self,
        command_parts: &'b [String],
        depth: usize,
    ) -> Pin<Box<dyn Future<Output = Result<CommandHelp>> + 'b>> {
        Box::pin(self.crawl_recursive_inner(command_parts, depth))
    }

    async fn crawl_recursive_inner(&mut self, command_parts: &[String], depth: usize) -> Result<CommandHelp> {
        let cmd = command_parts[0].clone();
        let mut args = command_parts[1..].to_vec();
        args.push("--help".to_string());

        log::info!("Executing: {} {}", cmd, args.join(" "));
        let output = match execute_command(&cmd, &args).await {
            Ok(out) => out,
            Err(e) => {
                log::warn!("Failed to execute {} {:?}: {}", cmd, args, e);
                let mut fallback_args = args.clone();
                if let Some(last) = fallback_args.last_mut() {
                    *last = "-h".to_string();
                }
                match execute_command(&cmd, &fallback_args).await {
                    Ok(out) => out,
                    Err(e) => {
                        log::error!("Failed to execute {} {:?} with -h: {}", cmd, fallback_args, e);
                        return Err(anyhow!("Command execution failed: {}", e));
                    }
                }
            }
        };
        debug!("{output:?}");

        let help_text = if !output.stdout.trim().is_empty() {
            output.stdout.clone()
        } else {
            output.stderr.clone()
        };

        if help_text.trim().is_empty() {
            log::warn!("Command {} {} returned empty output", cmd, args.join(" "));
            return Err(anyhow!("Empty help output for {} {}", cmd, args.join(" ")));
        }

        let mut node = CommandHelp {
            full_command: command_parts.to_vec(),
            help_text: help_text.clone(),
            children: Vec::new(),
        };

        if depth < MAX_DEPTH {
            let cmd_str = command_parts.join(" ");
            let prompt = prompt::build_subcommand_prompt(&cmd_str, &help_text);

            match self.llm_client.chat(prompt).await {
                Ok(response) => {
                    let subcommands = prompt::parse_subcommands_response(&response);
                    log::info!("Found subcommands for {}: {:?}", cmd_str, subcommands);

                    for sub in subcommands {
                        let mut sub_parts = command_parts.to_vec();
                        sub_parts.push(sub.command);
                        if let Ok(child_node) = self.crawl_recursive(&sub_parts, depth + 1).await {
                            node.children.push(child_node);
                        }
                    }
                }
                Err(e) => {
                    log::warn!("LLM parsing failed for subcommands of {}: {}", cmd_str, e);
                }
            }
        }

        Ok(node)
    }

    pub fn flatten(root: &CommandHelp) -> Vec<FlatCommand> {
        let mut result = Vec::new();
        Self::flatten_recursive(root, &mut result);
        result
    }

    fn flatten_recursive(node: &CommandHelp, result: &mut Vec<FlatCommand>) {
        result.push(FlatCommand {
            full_command: node.full_command.clone(),
            help_text: node.help_text.clone(),
        });
        for child in &node.children {
            Self::flatten_recursive(child, result);
        }
    }
}
