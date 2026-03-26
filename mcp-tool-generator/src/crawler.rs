use crate::llm_client::LlmClient;
use crate::prompt;
use crate::types::{CommandHelp};
use anyhow::{Result, anyhow, bail};
use log::{debug, error, trace};
use std::sync::Arc;
use tokio::process::Command;
use tokio::sync::Semaphore;

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

pub struct HelpCrawler<'a> {
    llm_client: &'a LlmClient,
}

impl<'a> HelpCrawler<'a> {
    pub fn new(llm_client: &'a LlmClient) -> Self {
        Self { llm_client }
    }

    pub async fn crawl(&mut self, command: &str) -> Result<Vec<CommandHelp>> {
        let commands: Vec<&str> = command.split_whitespace().collect();
        Ok(match commands.len() {
            1 => {
                self.crawl_command(&[commands[0].to_string()]).await?
            }
            2 => {
                vec![crawl_subcommand(&[commands[0].to_string(), commands[1].to_string()]).await?]
            }
            _ => {
                bail!(format!("Unsupport number of arguments: {}", commands.len()));
            }
        })
    }

    pub async fn crawl_command(&mut self, command_parts: &[String; 1]) -> Result<Vec<CommandHelp>> {
        let node = crawl(&command_parts[0..]).await?;
        let cmd_str = command_parts.join(" ");
        debug!("command_parts {cmd_str}");
        let prompt = prompt::build_subcommand_prompt(&cmd_str, &node.help_text);
        let mut nodes = vec![node];
        match self.llm_client.chat(prompt).await {
            Ok(response) => {
                let subcommands = prompt::parse_subcommands_response(&response);
                log::info!("Found subcommands for {}: {:?}", cmd_str, subcommands);

                if !subcommands.is_empty() {
                    return Ok(nodes);
                }
                nodes = vec![];
                let sem = Arc::new(Semaphore::new(5)); // 同时最多 3 个
                let mut handles = vec![];
                for sub in subcommands {
                    let sem = sem.clone();
                    let subcommand = [command_parts[0].clone(), sub.command];
                    let handle = tokio::spawn(async move {
                        // 获取令牌（没有就等待）
                        let permit = sem.acquire_owned().await.unwrap();
                        let rs = crawl_subcommand(&subcommand).await;
                        drop(permit); // 释放令牌（其实自动 drop）
                        rs
                    });
                    handles.push(handle);
                }
                for h in handles {
                    match h.await {
                        Ok(Ok(ch)) => {
                            nodes.push(ch);
                        }
                        Err(err) => {
                            error!("Failed to crawl subcommand {}: {}", cmd_str, err);
                        }
                        Ok(Err(err)) => {
                            error!("Failed to crawl subcommand {}: {}", cmd_str, err);
                        }
                    }
                }
            }
            Err(e) => {
                log::warn!("LLM parsing failed for subcommands of {}: {}", cmd_str, e);
            }
        }

        Ok(nodes)
    }

}

pub async fn crawl_subcommand(command_parts: &[String; 2]) -> Result<CommandHelp> {
    crawl(&command_parts[0..]).await
}

async fn crawl(command_parts: &[String]) -> Result<CommandHelp> {
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
    trace!("{output:?}");

    let help_text = if !output.stdout.trim().is_empty() {
        output.stdout.clone()
    } else {
        output.stderr.clone()
    };
    if help_text.trim().is_empty() {
        log::warn!("Command {} {} returned empty output", cmd, args.join(" "));
        return Err(anyhow!("Empty help output for {} {}", cmd, args.join(" ")));
    }
    let node = CommandHelp {
        full_command: command_parts.to_vec(),
        help_text: help_text.clone(),
    };
    Ok(node)
}
